//! Parsing the model's answer into variants and mapping them to a draft.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use postit_core::{
    AccountId, EmailBlock, EmailDocument, EmailLanguage, MediaItem, Post, PostKind, PostOverride,
    PostOverrides, ReplyTo,
};
use serde::{Deserialize, Deserializer};
use url::Url;

use crate::prompt::TargetBrief;
use crate::send::{extract_json, strip_think};

/// Reads `null` as the type's default, so a model that writes `"tags": null` is understood.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// An email as the model wrote it. `Debug` shows sizes only.
#[derive(Clone, Default, Deserialize)]
pub struct RawEmail {
    /// Subject line.
    #[serde(default, deserialize_with = "null_default")]
    pub subject: String,
    /// Preview text.
    #[serde(default, deserialize_with = "null_default")]
    pub preheader: String,
    /// Body paragraphs.
    #[serde(default, deserialize_with = "null_default")]
    pub paragraphs: Vec<String>,
}

/// One variant as the model wrote it. `Debug` shows the target and sizes only.
#[derive(Clone, Deserialize)]
pub struct RawVariant {
    /// The target's account ID.
    pub target: String,
    /// The text.
    #[serde(default, deserialize_with = "null_default")]
    pub text: String,
    /// Hashtag words.
    #[serde(default, deserialize_with = "null_default")]
    pub tags: Vec<String>,
    /// The email, for an email target.
    #[serde(default)]
    pub email: Option<RawEmail>,
}

#[derive(Deserialize)]
struct Wire {
    variants: Vec<serde_json::Value>,
}

/// The answer was not usable JSON in the expected shape.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// No JSON could be found in the answer.
    #[error("the answer holds no JSON")]
    NoJson,
    /// The JSON was not `{"variants": [...]}`.
    #[error("the answer is not in the expected shape")]
    WrongShape,
}

/// The variants that matched a requested target, and the targets with none.
#[derive(Clone)]
pub struct Parsed {
    /// One variant per answered target.
    pub variants: BTreeMap<AccountId, RawVariant>,
    /// Requested targets the model did not answer, in request order.
    pub missing: Vec<AccountId>,
}

impl Parsed {
    /// Gives each target this answer left out the variant `earlier` has for it, if any.
    pub fn fill_from(&mut self, earlier: &Self) {
        let variants = &mut self.variants;
        self.missing
            .retain(|account| match earlier.variants.get(account) {
                Some(variant) => {
                    variants.insert(*account, variant.clone());
                    false
                }
                None => true,
            });
    }
}

/// Cleans hashtag words: a leading `#` removed, blanks dropped, case-insensitive duplicates
/// removed (the first spelling wins).
fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for tag in tags {
        let clean = tag.trim().trim_start_matches('#').trim();
        if !clean.is_empty() && seen.insert(clean.to_lowercase()) {
            out.push(clean.to_owned());
        }
    }
    out
}

/// Parses an answer. Strips a leading `<think>` block and a code fence first. An unknown target
/// is dropped (so is a variant that is malformed, which leaves its target in `missing`), a
/// `null` field reads as empty, a repeated target keeps its first variant, and a variant's
/// email is kept only for an email target.
///
/// # Errors
/// [`ParseError`] when the answer is not JSON in the `variants` shape.
pub fn parse(text: &str, targets: &[TargetBrief]) -> Result<Parsed, ParseError> {
    let json = extract_json(strip_think(text)).ok_or(ParseError::NoJson)?;
    let wire: Wire = serde_json::from_str(json).map_err(|_| ParseError::WrongShape)?;
    let mut variants = BTreeMap::new();
    for value in wire.variants {
        // One malformed variant leaves its target unanswered; it does not fail the answer.
        let Ok(mut variant) = serde_json::from_value::<RawVariant>(value) else {
            continue;
        };
        let wanted = variant.target.trim();
        let Some(brief) = targets
            .iter()
            .find(|t| t.account.to_string().eq_ignore_ascii_case(wanted))
        else {
            continue;
        };
        if variants.contains_key(&brief.account) {
            continue;
        }
        variant.tags = normalize_tags(std::mem::take(&mut variant.tags));
        if !brief.is_email {
            variant.email = None;
        }
        variants.insert(brief.account, variant);
    }
    let missing = targets
        .iter()
        .map(|t| t.account)
        .filter(|a| !variants.contains_key(a))
        .collect();
    Ok(Parsed { variants, missing })
}

/// A draft in the shape D3's post create takes: the shared post and each target's override.
/// `Debug` shows counts only, never the drafted text.
#[derive(Clone)]
pub struct Draft {
    /// The shared post: the first target's text and tags, the media, the link, and the AI
    /// disclosure.
    pub post: Post,
    /// One override per requested target. A target the model did not answer gets empty text so
    /// validation reports it.
    pub overrides: PostOverrides,
}

impl fmt::Debug for RawEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawEmail")
            .field("subject_chars", &self.subject.chars().count())
            .field("preheader_chars", &self.preheader.chars().count())
            .field("paragraphs", &self.paragraphs.len())
            .finish()
    }
}

impl fmt::Debug for RawVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawVariant")
            .field("target", &self.target)
            .field("text_chars", &self.text.chars().count())
            .field("tags", &self.tags.len())
            .field("email", &self.email)
            .finish()
    }
}

impl fmt::Debug for Parsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parsed")
            .field("variants", &self.variants)
            .field("missing", &self.missing)
            .finish()
    }
}

impl fmt::Debug for Draft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Draft")
            .field("kind", &self.post.kind)
            .field("text_chars", &self.post.text.chars().count())
            .field("media", &self.post.media.len())
            .field("has_link", &self.post.link.is_some())
            .field("tags", &self.post.tags.len())
            .field("has_reply_to", &self.post.reply_to.is_some())
            .field("overrides", &self.overrides.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn email_document(variant: Option<&RawVariant>, language: EmailLanguage) -> EmailDocument {
    let (subject, preheader, mut paragraphs) = match variant.and_then(|v| v.email.clone()) {
        Some(email) => (email.subject, email.preheader, email.paragraphs),
        // An email target answered without an email: the text stands in as the body.
        None => (
            String::new(),
            String::new(),
            variant.map(|v| vec![v.text.clone()]).unwrap_or_default(),
        ),
    };
    paragraphs.retain(|p| !p.trim().is_empty());
    EmailDocument {
        subject,
        preheader: Some(preheader).filter(|p| !p.trim().is_empty()),
        language,
        blocks: paragraphs
            .into_iter()
            .map(|text| EmailBlock::Text { text })
            .collect(),
    }
}

/// Maps parsed variants to a draft. Every requested target gets an override.
pub fn to_draft(
    parsed: &Parsed,
    targets: &[TargetBrief],
    media: &[MediaItem],
    link: Option<Url>,
    reply_to: Option<ReplyTo>,
    language: impl Fn(AccountId) -> EmailLanguage,
) -> Draft {
    let mut overrides = PostOverrides::new();
    for brief in targets {
        let variant = parsed.variants.get(&brief.account);
        overrides.insert(
            brief.account,
            PostOverride {
                text: Some(variant.map(|v| v.text.clone()).unwrap_or_default()),
                tags: Some(variant.map(|v| v.tags.clone()).unwrap_or_default()),
                email: brief
                    .is_email
                    .then(|| email_document(variant, language(brief.account))),
                ..PostOverride::default()
            },
        );
    }
    // The shared post takes the first answered variant in request order.
    let first = targets.iter().find_map(|t| parsed.variants.get(&t.account));
    Draft {
        post: Post {
            kind: if reply_to.is_some() {
                PostKind::Comment
            } else {
                PostKind::Post
            },
            text: first.map(|v| v.text.clone()).unwrap_or_default(),
            media: media.to_vec(),
            link,
            tags: first.map(|v| v.tags.clone()).unwrap_or_default(),
            disclose_ai_generated: true,
            reply_to,
        },
        overrides,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::{
        AccountId, EmailBlock, EmailLanguage, MediaAssetId, MediaItem, PostKind, ReplyTo,
        TextMeasure,
    };
    use url::Url;
    use uuid::Uuid;

    use super::*;
    use crate::prompt::TargetBrief;

    fn id(n: u128) -> AccountId {
        AccountId::from_uuid(Uuid::from_u128(n))
    }

    fn brief(n: u128, is_email: bool) -> TargetBrief {
        TargetBrief {
            account: id(n),
            platform_name: "P".to_owned(),
            is_email,
            text_max: 100,
            measure: TextMeasure::Graphemes,
            includes_link_and_tags: false,
            style: None,
            language: None,
        }
    }

    fn answer(items: &[serde_json::Value]) -> String {
        serde_json::json!({ "variants": items }).to_string()
    }

    fn variant(n: u128, text: &str) -> serde_json::Value {
        serde_json::json!({ "target": id(n).to_string(), "text": text, "tags": [] })
    }

    #[test]
    fn parse_drops_unknown_keeps_first_reports_missing() {
        let targets = [brief(1, false), brief(2, false), brief(3, false)];
        let raw = answer(&[
            variant(1, "first one"),
            variant(2, "two"),
            variant(1, "repeat of one"),
            variant(9, "invented"),
        ]);
        let parsed = parse(&raw, &targets).unwrap();
        assert_eq!(parsed.missing, vec![id(3)]);
        assert_eq!(parsed.variants.len(), 2);
        assert_eq!(parsed.variants[&id(1)].text, "first one");
        assert!(!parsed.variants.contains_key(&id(9)));

        let draft = to_draft(&parsed, &targets, &[], None, None, |_| EmailLanguage::EnUs);
        // Every requested target comes back, the missing one empty so validation flags it.
        assert_eq!(draft.overrides.len(), 3);
        assert_eq!(draft.overrides[&id(3)].text.as_deref(), Some(""));
        assert_eq!(draft.overrides[&id(1)].text.as_deref(), Some("first one"));
        assert_eq!(draft.post.text, "first one");
    }

    #[test]
    fn parse_fenced_and_think_wrapped() {
        let targets = [brief(1, false)];
        let body = answer(&[variant(1, "hi")]);
        let fenced = format!("Here you go:\n```json\n{body}\n```\n");
        assert_eq!(parse(&fenced, &targets).unwrap().variants.len(), 1);
        let think = format!("<think>hmm {{not json}}</think>\n{body}");
        assert_eq!(parse(&think, &targets).unwrap().variants.len(), 1);
        let both = format!("<think>x</think>\n```\n{body}\n```");
        assert_eq!(parse(&both, &targets).unwrap().variants.len(), 1);
    }

    #[test]
    fn parse_garbage_is_error() {
        let targets = [brief(1, false)];
        assert!(parse("I cannot help with that.", &targets).is_err());
        assert!(parse("{\"variants\": 3}", &targets).is_err());
        assert!(parse("{\"other\": []}", &targets).is_err());
        assert!(parse("", &targets).is_err());
        // Cut off mid-JSON.
        assert!(parse("{\"variants\": [{\"target\": \"", &targets).is_err());
    }

    #[test]
    fn tags_normalized() {
        let targets = [brief(1, false)];
        let raw = serde_json::json!({"variants": [{
            "target": id(1).to_string(),
            "text": "x",
            "tags": ["#Bread", "bread", " ##Cakes ", "", "#", "BREAD", "fresh"],
        }]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        assert_eq!(
            parsed.variants[&id(1)].tags,
            vec!["Bread", "Cakes", "fresh"]
        );
    }

    #[test]
    fn to_draft_sets_disclosure_media_link_and_email_blocks() {
        let targets = [brief(1, false), brief(2, true)];
        let raw = serde_json::json!({"variants": [
            {"target": id(1).to_string(), "text": "post text", "tags": ["a"]},
            {"target": id(2).to_string(), "text": "summary", "tags": [],
             "email": {"subject": "S", "preheader": "P", "paragraphs": ["one", "", "two"]}},
        ]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        let media = vec![MediaItem {
            asset: MediaAssetId::from_uuid(Uuid::from_u128(77)),
            alt_text: None,
        }];
        let link = Url::parse("https://example.com/").unwrap();
        let draft = to_draft(&parsed, &targets, &media, Some(link.clone()), None, |a| {
            if a == id(2) {
                EmailLanguage::Ar
            } else {
                EmailLanguage::EnUs
            }
        });
        assert!(draft.post.disclose_ai_generated);
        assert_eq!(draft.post.kind, PostKind::Post);
        assert_eq!(draft.post.media, media);
        assert_eq!(draft.post.link, Some(link));
        assert_eq!(draft.post.tags, vec!["a"]);
        assert!(draft.overrides[&id(1)].email.is_none());
        assert_eq!(draft.overrides[&id(1)].tags, Some(vec!["a".to_owned()]));
        let doc = draft.overrides[&id(2)].email.as_ref().unwrap();
        assert_eq!(doc.subject, "S");
        assert_eq!(doc.preheader.as_deref(), Some("P"));
        assert_eq!(doc.language, EmailLanguage::Ar);
        assert_eq!(
            doc.blocks,
            vec![
                EmailBlock::Text {
                    text: "one".to_owned()
                },
                EmailBlock::Text {
                    text: "two".to_owned()
                },
            ]
        );
        assert!(doc.media_ids().is_empty());
    }

    #[test]
    fn email_variant_on_a_social_target_is_ignored() {
        let targets = [brief(1, false)];
        let raw = serde_json::json!({"variants": [
            {"target": id(1).to_string(), "text": "t", "tags": [],
             "email": {"subject": "S", "preheader": "", "paragraphs": ["p"]}},
        ]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        let draft = to_draft(&parsed, &targets, &[], None, None, |_| EmailLanguage::EnUs);
        assert!(draft.overrides[&id(1)].email.is_none());
    }

    #[test]
    fn to_draft_reply_is_comment_with_reply_to() {
        let targets = [brief(1, false)];
        let parsed = parse(&answer(&[variant(1, "thanks!")]), &targets).unwrap();
        let reply = ReplyTo {
            remote_post_id: "p1".to_owned(),
            remote_comment_id: Some("c1".to_owned()),
            url: None,
        };
        let draft = to_draft(&parsed, &targets, &[], None, Some(reply.clone()), |_| {
            EmailLanguage::EnUs
        });
        assert_eq!(draft.post.kind, PostKind::Comment);
        assert_eq!(draft.post.reply_to, Some(reply));
        assert!(draft.post.disclose_ai_generated);
    }

    #[test]
    fn null_fields_read_as_empty() {
        let targets = [brief(1, false), brief(2, true)];
        let raw = serde_json::json!({"variants": [
            {"target": id(1).to_string(), "text": null, "tags": null},
            {"target": id(2).to_string(), "text": "s",
             "email": {"subject": null, "preheader": null, "paragraphs": null}},
        ]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        assert!(parsed.missing.is_empty());
        assert_eq!(parsed.variants[&id(1)].text, "");
        assert!(parsed.variants[&id(1)].tags.is_empty());
        assert!(parsed.variants[&id(2)].email.is_some());
    }

    #[test]
    fn one_malformed_variant_is_missing_not_fatal() {
        let targets = [brief(1, false), brief(2, false)];
        let raw = serde_json::json!({"variants": [
            variant(1, "fine"),
            {"target": id(2).to_string(), "text": 42, "tags": []},
            "not an object",
        ]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        assert_eq!(parsed.missing, vec![id(2)]);
        assert_eq!(parsed.variants[&id(1)].text, "fine");
    }

    #[test]
    fn debug_hides_the_drafted_text() {
        let targets = [brief(1, true)];
        let raw = serde_json::json!({"variants": [
            {"target": id(1).to_string(), "text": "SECRET-TEXT", "tags": ["SECRETTAG"],
             "email": {"subject": "SECRET-SUBJECT", "preheader": "SECRET-PRE",
                       "paragraphs": ["SECRET-BODY"]}},
        ]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        let draft = to_draft(&parsed, &targets, &[], None, None, |_| EmailLanguage::EnUs);
        let shown = format!("{parsed:?} {draft:?}");
        assert!(!shown.contains("SECRET"), "{shown}");
        assert!(shown.contains("text_chars"), "{shown}");
    }

    #[test]
    fn shared_post_falls_back_to_the_first_answered_variant() {
        let targets = [brief(1, false), brief(2, false)];
        let raw = serde_json::json!({"variants": [
            {"target": id(2).to_string(), "text": "second", "tags": ["t"]},
        ]})
        .to_string();
        let parsed = parse(&raw, &targets).unwrap();
        let draft = to_draft(&parsed, &targets, &[], None, None, |_| EmailLanguage::EnUs);
        assert_eq!(draft.post.text, "second");
        assert_eq!(draft.post.tags, vec!["t"]);
        assert_eq!(draft.overrides[&id(1)].text.as_deref(), Some(""));
    }
}
