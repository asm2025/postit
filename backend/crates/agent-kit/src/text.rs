//! The text of a post as a platform that has no separate link or tag field shows it.

use postit_core::Variant;

/// The variant's text, then its link (unless the text already holds it), then its tags as
/// `#tag` (skipping any the text already holds, compared case-insensitively), each part on its
/// own paragraph. With no link and no tags to add, the text comes back unchanged.
///
/// An agent whose platform measures a post as one string (Telegram, Bluesky, Mastodon) sends
/// this and validates against it, so what is measured is what is sent.
#[must_use]
pub fn post_text(variant: &Variant) -> String {
    let lowered = variant.text.to_lowercase();
    let mut parts: Vec<String> = Vec::new();
    if !variant.text.is_empty() {
        parts.push(variant.text.clone());
    }
    if let Some(link) = &variant.link
        && !variant.text.contains(link.as_str())
    {
        parts.push(link.to_string());
    }
    let mut tags: Vec<String> = Vec::new();
    for tag in &variant.tags {
        let hashtag = format!("#{tag}");
        let key = hashtag.to_lowercase();
        if !lowered.contains(&key) && !tags.iter().any(|t| t.to_lowercase() == key) {
            tags.push(hashtag);
        }
    }
    if !tags.is_empty() {
        parts.push(tags.join(" "));
    }
    parts.join("\n\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use postit_core::{AccountId, AccountRef, PlatformId, PostKind};
    use serde_json::Map;
    use url::Url;
    use uuid::Uuid;

    fn variant(text: &str, link: Option<&str>, tags: &[&str]) -> Variant {
        Variant {
            target: AccountRef {
                platform: PlatformId::new("demo").unwrap(),
                account: AccountId::from_uuid(Uuid::nil()),
                remote_id: "r".to_owned(),
            },
            kind: PostKind::Post,
            text: text.to_owned(),
            media: Vec::new(),
            link: link.map(|l| Url::parse(l).unwrap()),
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options: Map::new(),
        }
    }

    #[test]
    fn composes_text_link_and_tags() {
        let a = "https://a.example/";
        let cases = [
            (variant("hi", None, &[]), "hi"),
            (variant("hi", Some(a), &[]), "hi\n\nhttps://a.example/"),
            (
                variant("see https://a.example/ ok", Some(a), &[]),
                "see https://a.example/ ok",
            ),
            (variant("hi", None, &["one", "two"]), "hi\n\n#one #two"),
            (
                variant("hi", Some(a), &["one"]),
                "hi\n\nhttps://a.example/\n\n#one",
            ),
            (
                variant("I like #Rust", None, &["rust", "go"]),
                "I like #Rust\n\n#go",
            ),
            (variant("", Some(a), &[]), "https://a.example/"),
            (variant("مرحبا بالعالم", None, &[]), "مرحبا بالعالم"),
        ];
        for (input, want) in cases {
            assert_eq!(post_text(&input), want, "{input:?}");
        }
    }

    #[test]
    fn repeated_tags_appear_once() {
        assert_eq!(post_text(&variant("hi", None, &["a", "A"])), "hi\n\n#a");
    }
}
