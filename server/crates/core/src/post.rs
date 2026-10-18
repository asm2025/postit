//! Posts, per-account overrides, and the resolved variants that get published.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use url::Url;

use crate::{AccountId, AccountRef, EmailDocument, MediaAssetId, PostId, Violation, ViolationCode};

/// Whether a post starts a conversation or answers one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PostKind {
    /// A standalone post.
    Post,
    /// A reply to an existing post or comment.
    Comment,
}

/// What a comment answers, in the platform's own terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplyTo {
    /// The platform's ID of the post being answered.
    pub remote_post_id: String,
    /// The platform's ID of the comment being answered, when replying to a comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_comment_id: Option<String>,
    /// A link to what is being answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
}

/// One media asset attached to a post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MediaItem {
    /// The stored asset.
    pub asset: MediaAssetId,
    /// Description for readers who cannot see the media.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_text: Option<String>,
}

/// The content shared by every target of a publish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Post {
    /// Standalone post or reply.
    pub kind: PostKind,
    /// The text.
    pub text: String,
    /// Attached media, in display order.
    #[serde(default)]
    pub media: Vec<MediaItem>,
    /// A link to attach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Url>,
    /// Hashtags, without the leading `#`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether the post says it was made with AI.
    #[serde(default)]
    pub disclose_ai_generated: bool,
    /// What a comment answers; required for [`PostKind::Comment`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyTo>,
}

impl Post {
    /// Violations of the rules tying `kind` to `reply_to`.
    #[must_use]
    pub fn kind_violations(&self) -> Vec<Violation> {
        kind_violations(self.kind, self.reply_to.as_ref())
    }
}

/// The rules tying a post's kind to its reply target, shared by posts and variants.
pub(crate) fn kind_violations(kind: PostKind, reply_to: Option<&ReplyTo>) -> Vec<Violation> {
    match (kind, reply_to) {
        (PostKind::Comment, None) => {
            vec![Violation::new(ViolationCode::ReplyToRequired).at("reply_to")]
        }
        (PostKind::Post, Some(_)) => {
            vec![Violation::new(ViolationCode::ReplyToNotAllowed).at("reply_to")]
        }
        _ => Vec::new(),
    }
}

/// Changes one account makes to the shared post. Absent fields keep the post's value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PostOverride {
    /// Replacement text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Replacement link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Url>,
    /// Replacement tags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// A subset of the post's media, in the order to publish it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Vec<MediaAssetId>>,
    /// The email document, for an email account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailDocument>,
    /// Values for the agent's options schema.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub options: Map<String, Value>,
}

/// Overrides keyed by the account they apply to.
pub type PostOverrides = BTreeMap<AccountId, PostOverride>;

/// The revision of a post, bumped on every edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(transparent)]
pub struct PostVersion(pub i64);

/// What one target publishes: the post with that account's override applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Variant {
    /// The account and platform it goes to.
    pub target: AccountRef,
    /// Standalone post or reply.
    pub kind: PostKind,
    /// The text.
    pub text: String,
    /// Attached media, in display order.
    pub media: Vec<MediaItem>,
    /// A link to attach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<Url>,
    /// Hashtags, without the leading `#`.
    pub tags: Vec<String>,
    /// Whether the post says it was made with AI.
    pub disclose_ai_generated: bool,
    /// What a comment answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<ReplyTo>,
    /// The email document, for an email account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailDocument>,
    /// Values for the agent's options schema.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub options: Map<String, Value>,
}

/// The frozen content of a publish: every variant as it was when the job was created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PublishSnapshot {
    /// The post this was taken from.
    pub post_id: PostId,
    /// The post revision it was taken at.
    pub post_version: PostVersion,
    /// One variant per target.
    pub variants: Vec<Variant>,
}

impl PublishSnapshot {
    /// The variant that goes to `account`.
    #[must_use]
    pub fn variant_for(&self, account: AccountId) -> Option<&Variant> {
        self.variants
            .iter()
            .find(|variant| variant.target.account == account)
    }
}

/// Applies `override_` to `post` for `target`.
///
/// Override fields replace the post's when present. A media override selects a subset of the
/// post's media, keeps each item's alt text, and takes the override's order. `kind`,
/// `reply_to`, and `disclose_ai_generated` always come from the post.
///
/// # Errors
/// One [`ViolationCode::OverrideMediaNotInPost`] per override media ID the post does not carry,
/// at `overrides.<account>.media[i]`.
pub fn resolve_variant(
    post: &Post,
    target: &AccountRef,
    override_: Option<&PostOverride>,
) -> Result<Variant, Vec<Violation>> {
    let mut variant = Variant {
        target: target.clone(),
        kind: post.kind,
        text: post.text.clone(),
        media: post.media.clone(),
        link: post.link.clone(),
        tags: post.tags.clone(),
        disclose_ai_generated: post.disclose_ai_generated,
        reply_to: post.reply_to.clone(),
        email: None,
        options: Map::new(),
    };
    let Some(over) = override_ else {
        return Ok(variant);
    };

    if let Some(text) = &over.text {
        variant.text.clone_from(text);
    }
    if let Some(link) = &over.link {
        variant.link = Some(link.clone());
    }
    if let Some(tags) = &over.tags {
        variant.tags.clone_from(tags);
    }
    variant.email.clone_from(&over.email);
    variant.options.clone_from(&over.options);

    if let Some(ids) = &over.media {
        let mut selected = Vec::with_capacity(ids.len());
        let mut violations = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            match post.media.iter().find(|item| item.asset == *id) {
                Some(item) => selected.push(item.clone()),
                None => violations.push(
                    Violation::new(ViolationCode::OverrideMediaNotInPost)
                        .at(format!("overrides.{}.media[{index}]", target.account))
                        .param("id", id.to_string()),
                ),
            }
        }
        if !violations.is_empty() {
            return Err(violations);
        }
        variant.media = selected;
    }
    Ok(variant)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::{EmailBlock, PlatformId};

    fn media(n: u128) -> MediaAssetId {
        MediaAssetId::from_uuid(Uuid::from_u128(n))
    }

    fn account(n: u128) -> AccountId {
        AccountId::from_uuid(Uuid::from_u128(n))
    }

    fn target(n: u128) -> AccountRef {
        AccountRef {
            platform: PlatformId::new("bluesky").unwrap(),
            account: account(n),
        }
    }

    fn post() -> Post {
        Post {
            kind: PostKind::Post,
            text: "hello".to_owned(),
            media: vec![
                MediaItem {
                    asset: media(1),
                    alt_text: Some("one".to_owned()),
                },
                MediaItem {
                    asset: media(2),
                    alt_text: None,
                },
                MediaItem {
                    asset: media(3),
                    alt_text: Some("three".to_owned()),
                },
            ],
            link: None,
            tags: vec!["a".to_owned()],
            disclose_ai_generated: true,
            reply_to: None,
        }
    }

    #[test]
    fn resolve_without_override_copies_post() {
        let p = post();
        let v = resolve_variant(&p, &target(9), None).unwrap();
        assert_eq!(v.target, target(9));
        assert_eq!(v.kind, p.kind);
        assert_eq!(v.text, p.text);
        assert_eq!(v.media, p.media);
        assert_eq!(v.tags, p.tags);
        assert!(v.disclose_ai_generated);
        assert!(v.email.is_none());
        assert!(v.options.is_empty());
    }

    #[test]
    fn override_replaces_fields() {
        let mut options = Map::new();
        options.insert("privacy".to_owned(), Value::from("public"));
        let over = PostOverride {
            text: Some("short".to_owned()),
            link: Some(Url::parse("https://example.com/").unwrap()),
            tags: Some(Vec::new()),
            options,
            ..PostOverride::default()
        };
        let v = resolve_variant(&post(), &target(9), Some(&over)).unwrap();
        assert_eq!(v.text, "short");
        assert_eq!(v.link.as_ref().unwrap().as_str(), "https://example.com/");
        assert!(v.tags.is_empty());
        assert_eq!(v.options["privacy"], "public");
        assert!(v.disclose_ai_generated);
        assert_eq!(v.media.len(), 3);
    }

    #[test]
    fn media_subset_keeps_alt_text_and_order() {
        let over = PostOverride {
            media: Some(vec![media(3), media(1)]),
            ..PostOverride::default()
        };
        let v = resolve_variant(&post(), &target(9), Some(&over)).unwrap();
        let assets: Vec<_> = v.media.iter().map(|m| m.asset).collect();
        assert_eq!(assets, vec![media(3), media(1)]);
        assert_eq!(v.media[0].alt_text.as_deref(), Some("three"));
        assert_eq!(v.media[1].alt_text.as_deref(), Some("one"));
    }

    #[test]
    fn media_not_in_post_is_violation() {
        let over = PostOverride {
            media: Some(vec![media(1), media(7)]),
            ..PostOverride::default()
        };
        let violations = resolve_variant(&post(), &target(9), Some(&over)).unwrap_err();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].code, ViolationCode::OverrideMediaNotInPost);
        assert_eq!(
            violations[0].path.as_deref(),
            Some(format!("overrides.{}.media[1]", account(9)).as_str())
        );
        assert_eq!(violations[0].params["id"], media(7).to_string());
    }

    #[test]
    fn email_override_carries_document() {
        let doc = EmailDocument {
            subject: "Sale".to_owned(),
            preheader: None,
            blocks: vec![EmailBlock::Divider],
        };
        let over = PostOverride {
            email: Some(doc.clone()),
            ..PostOverride::default()
        };
        let v = resolve_variant(&post(), &target(9), Some(&over)).unwrap();
        assert_eq!(v.email, Some(doc));
    }

    #[test]
    fn comment_needs_reply_to() {
        let mut p = post();
        p.kind = PostKind::Comment;
        let v = p.kind_violations();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, ViolationCode::ReplyToRequired);
        p.reply_to = Some(ReplyTo {
            remote_post_id: "42".to_owned(),
            remote_comment_id: None,
            url: None,
        });
        assert!(p.kind_violations().is_empty());
    }

    #[test]
    fn post_rejects_reply_to() {
        let mut p = post();
        p.reply_to = Some(ReplyTo {
            remote_post_id: "42".to_owned(),
            remote_comment_id: None,
            url: None,
        });
        let v = p.kind_violations();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code, ViolationCode::ReplyToNotAllowed);
    }

    fn snapshot() -> PublishSnapshot {
        let p = post();
        PublishSnapshot {
            post_id: PostId::from_uuid(Uuid::from_u128(100)),
            post_version: PostVersion(3),
            variants: vec![
                resolve_variant(&p, &target(1), None).unwrap(),
                resolve_variant(
                    &p,
                    &target(2),
                    Some(&PostOverride {
                        text: Some("other".to_owned()),
                        ..PostOverride::default()
                    }),
                )
                .unwrap(),
            ],
        }
    }

    #[test]
    fn snapshot_round_trips() {
        let s = snapshot();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<PublishSnapshot>(&json).unwrap(), s);
        assert_eq!(serde_json::to_value(PostVersion(3)).unwrap(), 3);
    }

    #[test]
    fn snapshot_variant_for() {
        let s = snapshot();
        assert_eq!(s.variant_for(account(2)).unwrap().text, "other");
        assert_eq!(s.variant_for(account(1)).unwrap().text, "hello");
        assert!(s.variant_for(account(3)).is_none());
    }
}
