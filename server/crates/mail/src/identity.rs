//! Renders the messages Zitadel posts to the identity mail webhook in the postit layout.

use std::fmt;

use askama::Template;
use postit_i18n::{Args, Locale, t};
use serde::Deserialize;
use url::Url;

use crate::error::MailError;
use crate::templates::Rendered;

/// One message from Zitadel's HTTP email provider. Unknown fields are ignored because Zitadel
/// adds fields across versions.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityMessage {
    /// What the message is about and who gets it.
    pub context_info: IdentityContext,
    /// The texts Zitadel already rendered in the user's language.
    #[serde(default)]
    pub template_data: IdentityTemplate,
    /// Raw values, such as the one-time code.
    #[serde(default)]
    pub args: IdentityArgs,
}

/// The `contextInfo` object of an identity message.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityContext {
    /// Zitadel's event type, for example `user.human.email.code.added`.
    pub event_type: String,
    /// The address the message goes to.
    pub recipient_email_address: String,
}

/// The `templateData` object: texts in the user's language, plus a link.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityTemplate {
    /// Heading text.
    pub title: Option<String>,
    /// Inbox preview text.
    pub pre_header: Option<String>,
    /// Subject line.
    pub subject: Option<String>,
    /// Greeting line.
    pub greeting: Option<String>,
    /// Body text; each line becomes a paragraph.
    pub text: Option<String>,
    /// The action link.
    pub url: Option<String>,
    /// Label of the action button.
    pub button_text: Option<String>,
    /// Footer text; a bare Zitadel translation key means none is configured.
    pub footer_text: Option<String>,
}

/// The `args` object of an identity message. Zitadel carries no language field.
#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityArgs {
    /// The one-time code, when the message carries one.
    pub code: Option<String>,
}

impl fmt::Debug for IdentityMessage {
    /// Prints only the event type: the address, code, link, and texts are secret or personal.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IdentityMessage")
            .field("event_type", &self.context_info.event_type)
            .finish_non_exhaustive()
    }
}

/// The action link: button label and the validated URL.
struct Link {
    label: String,
    href: String,
}

#[derive(Template)]
#[template(path = "identity.html")]
struct IdentityHtml<'a> {
    lang: &'a str,
    dir: &'a str,
    title: &'a str,
    pre_header: &'a str,
    footer: Vec<String>,
    heading: &'a str,
    greeting: &'a str,
    paragraphs: &'a [String],
    link: Option<&'a Link>,
    fallback: &'a str,
    code: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "identity.txt")]
struct IdentityText<'a> {
    heading: &'a str,
    greeting: &'a str,
    paragraphs: &'a [String],
    link: Option<&'a Link>,
    code: Option<&'a str>,
    footer: &'a [String],
}

fn is_arabic(c: char) -> bool {
    matches!(c,
        '\u{0600}'..='\u{06FF}'
        | '\u{0750}'..='\u{077F}'
        | '\u{08A0}'..='\u{08FF}'
        | '\u{FB50}'..='\u{FDFF}'
        | '\u{FE70}'..='\u{FEFF}')
}

/// A trimmed value, or `None` when it is missing or blank.
fn present(value: Option<&String>) -> Option<&str> {
    value.map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// Trimmed, non-empty lines of `text`.
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The locale of a message. Zitadel sends no language field but renders the texts in the
/// user's language, so this is `Ar` when most letters of the subject, greeting, and text are
/// Arabic script, and `EnUs` otherwise.
#[must_use]
pub fn identity_locale(message: &IdentityMessage) -> Locale {
    let data = &message.template_data;
    let (mut arabic, mut letters) = (0usize, 0usize);
    for c in [&data.subject, &data.greeting, &data.text]
        .into_iter()
        .flatten()
        .flat_map(|s| s.chars())
        .filter(|c| c.is_alphabetic())
    {
        letters += 1;
        if is_arabic(c) {
            arabic += 1;
        }
    }
    if arabic * 2 > letters {
        Locale::Ar
    } else {
        Locale::EnUs
    }
}

/// True for a bare Zitadel translation key such as `VerifyEmail.Footer`, which Zitadel sends
/// when no footer text is configured.
fn is_translation_key(text: &str) -> bool {
    text.split_once('.').is_some_and(|(a, b)| {
        let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic());
        word(a) && word(b)
    })
}

/// Footer lines, or none for a missing or bare-key footer.
fn footer_lines(text: Option<&String>) -> Vec<String> {
    match present(text) {
        Some(text) if !is_translation_key(text) => lines(text),
        _ => Vec::new(),
    }
}

/// Renders `message` in the postit layout, in the locale [`identity_locale`] detects.
///
/// # Errors
///
/// Returns [`MailError::Template`] if a template fails to render.
pub fn render_identity(message: &IdentityMessage) -> Result<Rendered, MailError> {
    let err = |e: askama::Error| MailError::Template(e.to_string());
    let data = &message.template_data;
    let locale = identity_locale(message);
    let (lang, dir) = (locale.tag(), locale.dir());

    let subject: String = present(data.subject.as_ref())
        .or_else(|| present(data.title.as_ref()))
        .unwrap_or("postit")
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n'))
        .collect();
    let heading = present(data.title.as_ref()).unwrap_or(&subject);
    let greeting = present(data.greeting.as_ref()).unwrap_or("");
    let paragraphs = lines(data.text.as_deref().unwrap_or(""));
    let link = present(data.url.as_ref())
        .and_then(|raw| Url::parse(raw).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|u| Link {
            label: present(data.button_text.as_ref())
                .unwrap_or(heading)
                .to_owned(),
            href: u.to_string(),
        });
    let code = present(message.args.code.as_ref());
    let footer = footer_lines(data.footer_text.as_ref());
    let fallback = t(locale, "identity-link-fallback", &Args::new());

    Ok(Rendered {
        subject: subject.clone(),
        text: IdentityText {
            heading,
            greeting,
            paragraphs: &paragraphs,
            link: link.as_ref(),
            code,
            footer: &footer,
        }
        .render()
        .map_err(err)?,
        html: IdentityHtml {
            lang,
            dir,
            title: &subject,
            pre_header: present(data.pre_header.as_ref()).unwrap_or(""),
            footer: footer.clone(),
            heading,
            greeting,
            paragraphs: &paragraphs,
            link: link.as_ref(),
            fallback: &fallback,
            code,
        }
        .render()
        .map_err(err)?,
    })
}
