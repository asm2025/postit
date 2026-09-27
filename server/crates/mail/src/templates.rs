use askama::Template;
use url::Url;

use crate::error::MailError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingUser {
    pub display_name: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailContent {
    UserPendingApproval {
        pending: Vec<PendingUser>,
        more: u64,
    },
    UserApproved {
        display_name: String,
    },
}

pub struct Rendered {
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[derive(Template)]
#[template(path = "user_pending_approval.txt")]
struct PendingText<'a> {
    labels: &'a [String],
    more: u64,
    review_url: &'a str,
}

#[derive(Template)]
#[template(path = "user_pending_approval.html")]
struct PendingHtml<'a> {
    labels: &'a [String],
    more: u64,
    review_url: &'a str,
}

#[derive(Template)]
#[template(path = "user_approved.txt")]
struct ApprovedText<'a> {
    display_name: &'a str,
    app_url: &'a str,
}

#[derive(Template)]
#[template(path = "user_approved.html")]
struct ApprovedHtml<'a> {
    display_name: &'a str,
    app_url: &'a str,
}

/// The admin review page under `app_url`. The base path gets a trailing slash first, so a
/// `public_url` like `https://host/app` keeps its last segment (`/app/admin/users`) instead
/// of `Url::join` replacing it.
fn pending_review_url(app_url: &Url) -> Result<Url, MailError> {
    let mut base = app_url.clone();
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base.join("admin/users?status=pending")
        .map_err(|e| MailError::Template(e.to_string()))
}

/// # Errors
///
/// Returns [`MailError::Template`] if a template fails to render.
pub fn render(content: &MailContent, app_url: &Url) -> Result<Rendered, MailError> {
    let err = |e: askama::Error| MailError::Template(e.to_string());
    match content {
        MailContent::UserPendingApproval { pending, more } => {
            let labels: Vec<String> = pending
                .iter()
                .map(|u| match &u.email {
                    Some(email) => format!("{} <{email}>", u.display_name),
                    None => u.display_name.clone(),
                })
                .collect();
            let total = u64::try_from(labels.len())
                .unwrap_or(u64::MAX)
                .saturating_add(*more);
            let review = pending_review_url(app_url)?;
            let review_url = review.as_str();
            Ok(Rendered {
                subject: format!("{total} postit account(s) waiting for approval"),
                text: PendingText {
                    labels: &labels,
                    more: *more,
                    review_url,
                }
                .render()
                .map_err(err)?,
                html: PendingHtml {
                    labels: &labels,
                    more: *more,
                    review_url,
                }
                .render()
                .map_err(err)?,
            })
        }
        MailContent::UserApproved { display_name } => Ok(Rendered {
            subject: "Your postit account was approved".into(),
            text: ApprovedText {
                display_name,
                app_url: app_url.as_str(),
            }
            .render()
            .map_err(err)?,
            html: ApprovedHtml {
                display_name,
                app_url: app_url.as_str(),
            }
            .render()
            .map_err(err)?,
        }),
    }
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
            "https://host/app/admin/users?status=pending"
        );
    }

    #[test]
    fn review_url_keeps_a_base_path_with_a_trailing_slash() {
        assert_eq!(
            review("https://host/app/"),
            "https://host/app/admin/users?status=pending"
        );
        assert_eq!(
            review("https://host"),
            "https://host/admin/users?status=pending"
        );
    }
}
