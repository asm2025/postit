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
            let review = app_url
                .join("admin/users?status=pending")
                .map_err(|e| MailError::Template(e.to_string()))?;
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
