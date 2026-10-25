use askama::Template;
use postit_i18n::{Args, Locale, t};
use url::Url;

use crate::error::MailError;

/// A user waiting for approval, as listed in an admin email.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUser {
    /// Name shown for the user.
    pub display_name: String,
    /// The user's email address, when known.
    pub email: Option<String>,
}

/// Typed content of an email, ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailContent {
    /// Admin digest of users waiting for approval.
    UserPendingApproval {
        /// The users listed in the email.
        pending: Vec<PendingUser>,
        /// How many further pending users the list omits.
        more: u64,
    },
    /// Notice to a user that their account was approved.
    UserApproved {
        /// Name the email greets the user by.
        display_name: String,
    },
}

/// A rendered email body.
pub struct Rendered {
    /// Subject line.
    pub subject: String,
    /// Plain-text body.
    pub text: String,
    /// HTML body.
    pub html: String,
}

/// One listed user: the name and optional address, kept apart so HTML can isolate each.
struct Label<'a> {
    name: &'a str,
    email: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "user_pending_approval.txt")]
struct PendingText<'a> {
    intro: &'a str,
    labels: &'a [String],
    more: &'a str,
    review_label: &'a str,
    review_url: &'a str,
    footer: &'a str,
}

#[derive(Template)]
#[template(path = "user_pending_approval.html")]
struct PendingHtml<'a> {
    lang: &'a str,
    dir: &'a str,
    title: &'a str,
    pre_header: &'a str,
    footer: Vec<String>,
    intro: &'a str,
    labels: &'a [Label<'a>],
    more: &'a str,
    review_label: &'a str,
    review_url: &'a str,
}

#[derive(Template)]
#[template(path = "user_approved.txt")]
struct ApprovedText<'a> {
    greeting: &'a str,
    body: &'a str,
    sign_in: &'a str,
    app_url: &'a str,
}

#[derive(Template)]
#[template(path = "user_approved.html")]
struct ApprovedHtml<'a> {
    lang: &'a str,
    dir: &'a str,
    title: &'a str,
    pre_header: &'a str,
    footer: Vec<String>,
    greeting_before: &'a str,
    display_name: &'a str,
    greeting_after: &'a str,
    body: &'a str,
    sign_in: &'a str,
    app_url: &'a str,
}

/// The admin review page under `app_url`. The base path gets a trailing slash first, so a
/// `public_url` like `https://host/app` keeps its last segment (`/app/users`) instead
/// of `Url::join` replacing it.
fn pending_review_url(app_url: &Url) -> Result<Url, MailError> {
    let mut base = app_url.clone();
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base.join("users?status=pending")
        .map_err(|e| MailError::Template(e.to_string()))
}

/// Placeholder for the display name while splitting a translated greeting around it.
const NAME_MARK: &str = "\u{1}name\u{1}";

/// Renders `content` in `locale`. Templates get only translated strings plus `lang`/`dir`.
///
/// # Errors
///
/// Returns [`MailError::Template`] if a template fails to render.
pub fn render(content: &MailContent, app_url: &Url, locale: Locale) -> Result<Rendered, MailError> {
    match content {
        MailContent::UserPendingApproval { pending, more } => {
            render_pending(pending, *more, app_url, locale)
        }
        MailContent::UserApproved { display_name } => {
            render_approved(display_name, app_url, locale)
        }
    }
}

fn render_pending(
    pending: &[PendingUser],
    more: u64,
    app_url: &Url,
    locale: Locale,
) -> Result<Rendered, MailError> {
    let err = |e: askama::Error| MailError::Template(e.to_string());
    let (lang, dir) = (locale.tag(), locale.dir());
    let texts: Vec<String> = pending
        .iter()
        .map(|u| match &u.email {
            Some(email) => format!("{} <{email}>", u.display_name),
            None => u.display_name.clone(),
        })
        .collect();
    let labels: Vec<Label<'_>> = pending
        .iter()
        .map(|u| Label {
            name: &u.display_name,
            email: u.email.as_deref(),
        })
        .collect();
    let more_n = i64::try_from(more).unwrap_or(i64::MAX);
    let total = i64::try_from(pending.len())
        .unwrap_or(i64::MAX)
        .saturating_add(more_n);
    let count = Args::new().num("count", total);
    let more_text = if more > 0 {
        t(locale, "pending-more", &Args::new().num("more", more_n))
    } else {
        String::new()
    };
    let none = Args::new();
    let intro = t(locale, "pending-intro", &count);
    let review_label = t(locale, "pending-review", &none);
    let footer = t(locale, "pending-footer", &none);
    let review = pending_review_url(app_url)?;
    let review_url = review.as_str();
    let subject = t(locale, "pending-subject", &count);
    let text = PendingText {
        intro: &intro,
        labels: &texts,
        more: &more_text,
        review_label: &review_label,
        review_url,
        footer: &footer,
    }
    .render()
    .map_err(err)?;
    let html = PendingHtml {
        lang,
        dir,
        title: &subject,
        pre_header: "",
        footer: vec![footer],
        intro: &intro,
        labels: &labels,
        more: &more_text,
        review_label: &review_label,
        review_url,
    }
    .render()
    .map_err(err)?;
    Ok(Rendered {
        subject,
        text,
        html,
    })
}

fn render_approved(
    display_name: &str,
    app_url: &Url,
    locale: Locale,
) -> Result<Rendered, MailError> {
    let err = |e: askama::Error| MailError::Template(e.to_string());
    let (lang, dir) = (locale.tag(), locale.dir());
    let none = Args::new();
    let body = t(locale, "approved-body", &none);
    let sign_in = t(locale, "approved-sign-in", &none);
    let greeting = t(
        locale,
        "approved-greeting",
        &Args::new().str("name", display_name),
    );
    let marked = t(
        locale,
        "approved-greeting",
        &Args::new().str("name", NAME_MARK),
    );
    let (before, after) = marked.split_once(NAME_MARK).unwrap_or((&marked, ""));
    let subject = t(locale, "approved-subject", &none);
    let text = ApprovedText {
        greeting: &greeting,
        body: &body,
        sign_in: &sign_in,
        app_url: app_url.as_str(),
    }
    .render()
    .map_err(err)?;
    let html = ApprovedHtml {
        lang,
        dir,
        title: &subject,
        pre_header: "",
        footer: Vec::new(),
        greeting_before: before,
        display_name,
        greeting_after: after,
        body: &body,
        sign_in: &sign_in,
        app_url: app_url.as_str(),
    }
    .render()
    .map_err(err)?;
    Ok(Rendered {
        subject,
        text,
        html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(base: &str) -> String {
        let base = Url::parse(base).unwrap_or_else(|e| unreachable!("parse: {e}"));
        pending_review_url(&base)
            .unwrap_or_else(|e| unreachable!("review url: {e}"))
            .to_string()
    }

    #[test]
    fn review_url_keeps_a_base_path_without_a_trailing_slash() {
        assert_eq!(
            review("https://host/app"),
            "https://host/app/users?status=pending"
        );
    }

    #[test]
    fn review_url_keeps_a_base_path_with_a_trailing_slash() {
        assert_eq!(
            review("https://host/app/"),
            "https://host/app/users?status=pending"
        );
        assert_eq!(review("https://host"), "https://host/users?status=pending");
    }

    fn render_in(content: &MailContent, locale: Locale) -> Rendered {
        let app = Url::parse("https://app.postit.test").unwrap_or_else(|e| unreachable!("{e}"));
        render(content, &app, locale).unwrap_or_else(|e| unreachable!("render: {e}"))
    }

    fn approved(name: &str) -> MailContent {
        MailContent::UserApproved {
            display_name: name.into(),
        }
    }

    #[test]
    fn renders_approved_in_arabic() {
        let rendered = render_in(&approved("Ada"), Locale::Ar);
        assert!(rendered.html.contains("lang=\"ar\""), "{}", rendered.html);
        assert!(rendered.html.contains("dir=\"rtl\""), "{}", rendered.html);
        assert_eq!(
            rendered.subject,
            t(Locale::Ar, "approved-subject", &Args::new())
        );
        assert!(!rendered.subject.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn renders_pending_in_english_with_plural() {
        let user = |n: &str| PendingUser {
            display_name: n.into(),
            email: None,
        };
        let content = MailContent::UserPendingApproval {
            pending: vec![user("Ada"), user("Bob")],
            more: 3,
        };
        let rendered = render_in(&content, Locale::EnUs);
        assert_eq!(
            rendered.subject,
            "5 postit accounts are waiting for approval"
        );
        assert!(rendered.html.contains("lang=\"en-US\""));
        assert!(rendered.html.contains("dir=\"ltr\""));
    }

    #[test]
    fn display_name_is_isolated() {
        let rendered = render_in(&approved("Ada"), Locale::EnUs);
        assert!(
            rendered.html.contains("<bdi>Ada</bdi>"),
            "{}",
            rendered.html
        );
    }
}
