//! postit's English and Arabic texts for Zitadel's identity messages (plan 03 D0c).
//!
//! The texts live in `docker/zitadel/messages/{en,ar}.yaml`, one top-level key per
//! message type. This module loads and validates a file and builds the admin API
//! requests that install it; it makes no network calls.
// The bootstrap step that sends these requests is wired in a later D0c task.
#![allow(dead_code, reason = "wired into zitadel-bootstrap by a later D0c task")]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Zitadel message types, in the path spelling the running Zitadel accepts.
pub const MESSAGE_TYPES: [&str; 8] = [
    "init",
    "verifyemail",
    "verifyemailotp",
    "passwordreset",
    "password_change",
    "passwordless_registration",
    "invite_user",
    "domainclaimed",
];

/// The only Go template variables a text may use.
const VARIABLES: [&str; 4] = [
    "{{.DisplayName}}",
    "{{.PreferredLoginName}}",
    "{{.Code}}",
    "{{.URL}}",
];

static PLACEHOLDER: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\{\{[^}]*\}\}").ok());

/// One message's texts, in the body shape of Zitadel's text endpoint.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageText {
    pub title: String,
    pub pre_header: String,
    pub subject: String,
    pub greeting: String,
    pub text: String,
    pub button_text: String,
    pub footer_text: String,
}

impl MessageText {
    fn fields(&self) -> [&str; 7] {
        [
            &self.title,
            &self.pre_header,
            &self.subject,
            &self.greeting,
            &self.text,
            &self.button_text,
            &self.footer_text,
        ]
    }
}

/// Template placeholders in `field` that are not one of the four allowed variables.
fn unknown_variables(field: &str) -> Vec<&str> {
    PLACEHOLDER.as_ref().map_or_else(Vec::new, |re| {
        re.find_iter(field)
            .map(|m| m.as_str())
            .filter(|found| !VARIABLES.contains(found))
            .collect()
    })
}

/// `{type -> MessageText}` for one language file. Rejects unknown fields and unknown
/// template variables.
pub fn load(path: &Path) -> Result<BTreeMap<String, MessageText>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let texts: BTreeMap<String, MessageText> =
        serde_norway::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    for (kind, text) in &texts {
        for field in text.fields() {
            let unknown = unknown_variables(field);
            if !unknown.is_empty() {
                bail!(
                    "{}: message {kind} uses unknown variables {unknown:?}",
                    path.display()
                );
            }
        }
    }
    Ok(texts)
}

/// (path, body) for each PUT: `/admin/v1/text/message/{type}/{language}`.
pub fn requests(
    language: &str,
    texts: &BTreeMap<String, MessageText>,
) -> Vec<(String, serde_json::Value)> {
    texts
        .iter()
        .filter_map(|(kind, text)| {
            let body = serde_json::to_value(text).ok()?;
            Some((format!("/admin/v1/text/message/{kind}/{language}"), body))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(language: &str) -> BTreeMap<String, MessageText> {
        let path = format!(
            "{}/../../docker/zitadel/messages/{language}.yaml",
            env!("CARGO_MANIFEST_DIR")
        );
        load(Path::new(&path)).unwrap_or_else(|e| unreachable!("{e:#}"))
    }

    fn is_arabic(c: char) -> bool {
        matches!(c, '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}')
    }

    fn write_temp(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let path = dir.path().join("bad.yaml");
        std::fs::write(&path, content).unwrap_or_else(|e| unreachable!("{e}"));
        (dir, path)
    }

    #[test]
    fn both_languages_cover_every_type() {
        let mut expected: Vec<&str> = MESSAGE_TYPES.to_vec();
        expected.sort_unstable();
        for language in ["en", "ar"] {
            let texts = messages(language);
            let keys: Vec<&str> = texts.keys().map(String::as_str).collect();
            assert_eq!(keys, expected, "{language}");
        }
    }

    #[test]
    fn only_known_variables() {
        let Some(re) = PLACEHOLDER.as_ref() else {
            unreachable!("placeholder regex compiles");
        };
        for language in ["en", "ar"] {
            for (kind, text) in &messages(language) {
                for field in text.fields() {
                    for found in re.find_iter(field) {
                        assert!(
                            VARIABLES.contains(&found.as_str()),
                            "{language}/{kind}: {}",
                            found.as_str()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn arabic_texts_are_arabic() {
        let (en, ar) = (messages("en"), messages("ar"));
        for (kind, ar_text) in &ar {
            let en_text = &en[kind];
            for (ar_field, en_field) in ar_text.fields().into_iter().zip(en_text.fields()) {
                if ar_field.is_empty() {
                    continue;
                }
                assert!(ar_field.chars().any(is_arabic), "{kind}: {ar_field}");
                assert_ne!(ar_field, en_field, "{kind}");
            }
        }
    }

    #[test]
    fn requests_build_admin_paths() {
        let built = requests("ar", &messages("ar"));
        assert_eq!(built.len(), MESSAGE_TYPES.len());
        let (_, body) = built
            .iter()
            .find(|(path, _)| path == "/admin/v1/text/message/verifyemail/ar")
            .unwrap_or_else(|| unreachable!("verifyemail request"));
        assert!(body.get("buttonText").is_some());
        assert!(body.get("preHeader").is_some());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let (_dir, path) = write_temp(
            "init:\n  title: a\n  preHeader: a\n  subject: a\n  greeting: a\n  text: a\n  buton: x\n  footerText: a\n",
        );
        assert!(load(&path).is_err());
    }

    #[test]
    fn unknown_variable_is_rejected() {
        let (_dir, path) = write_temp(
            "init:\n  title: '{{.Nope}}'\n  preHeader: a\n  subject: a\n  greeting: a\n  text: a\n  buttonText: a\n  footerText: a\n",
        );
        assert!(load(&path).is_err());
    }
}
