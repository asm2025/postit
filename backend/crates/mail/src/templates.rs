use askama::Template;
use postit_core::{AccountLabel, CommentsNotice, Notice, ParkReason, SkipReason};
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
    /// The user's critical notifications not yet emailed, oldest first, one line each.
    NotificationAlert {
        /// The notices listed in the email.
        notices: Vec<Notice>,
    },
    /// The daily digest of new comments: one entry per account, counts already summed.
    CommentDigest {
        /// The accounts that got comments, in the order they were first commented on.
        comments: Vec<CommentsNotice>,
    },
}

/// The most lines an alert or digest lists; the rest become one "and N more" line.
pub const MAX_LISTED_LINES: usize = 20;

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
    app_base(app_url)
        .join("users?status=pending")
        .map_err(|e| MailError::Template(e.to_string()))
}

/// `app_url` with a trailing slash, so a relative join keeps its last segment.
fn app_base(app_url: &Url) -> Url {
    let mut base = app_url.clone();
    if !base.path().ends_with('/') {
        let path = format!("{}/", base.path());
        base.set_path(&path);
    }
    base
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
        MailContent::NotificationAlert { notices } => render_alert(notices, app_url, locale),
        MailContent::CommentDigest { comments } => render_digest(comments, app_url, locale),
    }
}

/// One alert line, split around its label so HTML can isolate the label.
struct AlertLine {
    before: String,
    label: String,
    after: String,
}

#[derive(Template)]
#[template(path = "notification_alert.txt")]
struct AlertText<'a> {
    intro: &'a str,
    lines: &'a [String],
    more: &'a str,
    open_label: &'a str,
    open_url: &'a str,
    footer: &'a str,
}

#[derive(Template)]
#[template(path = "notification_alert.html")]
struct AlertHtml<'a> {
    lang: &'a str,
    dir: &'a str,
    title: &'a str,
    pre_header: &'a str,
    footer: Vec<String>,
    intro: &'a str,
    lines: &'a [AlertLine],
    more: &'a str,
    open_label: &'a str,
    open_url: &'a str,
}

/// Placeholder for the label while splitting a translated line around it.
const LABEL_MARK: &str = "\u{1}label\u{1}";

fn park_reason_id(reason: Option<ParkReason>) -> &'static str {
    match reason {
        Some(ParkReason::OwnerDisabled) => "park-reason-owner_disabled",
        Some(ParkReason::AccountNeedsReconnect) => "park-reason-account_needs_reconnect",
        Some(ParkReason::PlatformUnavailable) => "park-reason-platform_unavailable",
        Some(ParkReason::PlatformDisabled) => "park-reason-platform_disabled",
        Some(ParkReason::LivePublishDisabled) => "park-reason-live_publish_disabled",
        Some(ParkReason::QuotaExceeded) => "park-reason-quota_exceeded",
        Some(ParkReason::OutcomeUnresolvable) => "park-reason-outcome_unresolvable",
        None => "park-reason-unknown",
    }
}

fn skip_reason_id(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::AiProviderMissing => "skip-reason-ai_provider_missing",
        SkipReason::RewriteInvalid => "skip-reason-rewrite_invalid",
        SkipReason::JobActive => "skip-reason-job_active",
    }
}

/// The label a notice's line names: the account's display name, or the other party's name
/// for a delegation notice.
fn notice_label(notice: &Notice) -> Option<&str> {
    match notice {
        Notice::DeliveryDeadLettered(n)
        | Notice::DeliveryFailed(n)
        | Notice::DeliveryOutcomeUnknown(n)
        | Notice::DeliveryParked(n)
        | Notice::DeliveryStale(n)
        | Notice::DeliveryPublished(n) => Some(&n.account.account_name),
        Notice::AccountNeedsReconnect(n) | Notice::AccountRevokedByPlatform(n) => {
            Some(&n.account.account_name)
        }
        Notice::ReconnectByPassed(n) | Notice::ReconnectReminder(n) => {
            Some(&n.account.account_name)
        }
        Notice::NewComments(n) => Some(&n.account.account_name),
        Notice::DelegationChanged(n) => Some(&n.party_name),
        Notice::JobPartial(_) | Notice::RecurrenceSkipped(_) | Notice::UserPending(_) => None,
    }
}

/// The arguments of a notice's line, with `label` standing for its label: `$account`
/// (`$name` for a delegation notice), `$platform`, `$count`, and `$reason`, translated.
fn notice_args(notice: &Notice, locale: Locale, label: &str) -> Args {
    let account = |a: &AccountLabel| {
        Args::new()
            .str("account", label)
            .str("platform", a.platform.as_str())
    };
    match notice {
        Notice::DeliveryParked(n) | Notice::DeliveryStale(n) => account(&n.account).str(
            "reason",
            t(locale, park_reason_id(n.park_reason), &Args::new()),
        ),
        Notice::DeliveryDeadLettered(n)
        | Notice::DeliveryFailed(n)
        | Notice::DeliveryOutcomeUnknown(n)
        | Notice::DeliveryPublished(n) => account(&n.account),
        Notice::AccountNeedsReconnect(n) | Notice::AccountRevokedByPlatform(n) => {
            account(&n.account)
        }
        Notice::ReconnectByPassed(n) | Notice::ReconnectReminder(n) => account(&n.account),
        Notice::NewComments(n) => account(&n.account).num("count", i64::from(n.count)),
        Notice::JobPartial(n) => Args::new().num("count", i64::from(n.failed)),
        Notice::RecurrenceSkipped(n) => {
            Args::new().str("reason", t(locale, skip_reason_id(n.reason), &Args::new()))
        }
        Notice::DelegationChanged(_) => Args::new().str("name", label),
        Notice::UserPending(_) => Args::new(),
    }
}

/// A notice's line in `locale`, from its `notice-<kind>` message: whole for the text body,
/// and split around its label for HTML.
fn notice_line(notice: &Notice, locale: Locale) -> (String, AlertLine) {
    let id = format!("notice-{}", notice.kind().as_str());
    let label = notice_label(notice).unwrap_or_default();
    let text = t(locale, &id, &notice_args(notice, locale, label));
    let marked = t(locale, &id, &notice_args(notice, locale, LABEL_MARK));
    let line = match marked.split_once(LABEL_MARK) {
        Some((before, after)) => AlertLine {
            before: before.to_owned(),
            label: label.to_owned(),
            after: after.to_owned(),
        },
        None => AlertLine {
            before: marked,
            label: String::new(),
            after: String::new(),
        },
    };
    (text, line)
}

/// The "and N more" line for `hidden` lines past [`MAX_LISTED_LINES`], empty when none are.
fn more_line(locale: Locale, hidden: usize) -> String {
    if hidden == 0 {
        return String::new();
    }
    let more = i64::try_from(hidden).unwrap_or(i64::MAX);
    t(locale, "list-more", &Args::new().num("more", more))
}

/// Everything the alert and digest renderers share once their lines are built.
struct ListMail<'a> {
    subject: String,
    intro: String,
    open_label: String,
    footer: String,
    texts: &'a [String],
    lines: &'a [AlertLine],
    more: String,
}

fn render_list(list: &ListMail<'_>, app_url: &Url, locale: Locale) -> Result<Rendered, MailError> {
    let err = |e: askama::Error| MailError::Template(e.to_string());
    let (lang, dir) = (locale.tag(), locale.dir());
    let open = app_base(app_url);
    let open_url = open.as_str();
    let text = AlertText {
        intro: &list.intro,
        lines: list.texts,
        more: &list.more,
        open_label: &list.open_label,
        open_url,
        footer: &list.footer,
    }
    .render()
    .map_err(err)?;
    let html = AlertHtml {
        lang,
        dir,
        title: &list.subject,
        pre_header: "",
        intro: &list.intro,
        lines: list.lines,
        more: &list.more,
        open_label: &list.open_label,
        open_url,
        footer: vec![list.footer.clone()],
    }
    .render()
    .map_err(err)?;
    Ok(Rendered {
        subject: list.subject.clone(),
        text,
        html,
    })
}

fn render_alert(notices: &[Notice], app_url: &Url, locale: Locale) -> Result<Rendered, MailError> {
    let shown = notices.len().min(MAX_LISTED_LINES);
    let (texts, lines): (Vec<String>, Vec<AlertLine>) = notices
        .iter()
        .take(shown)
        .map(|n| notice_line(n, locale))
        .unzip();
    let count = Args::new().num("count", i64::try_from(notices.len()).unwrap_or(i64::MAX));
    let none = Args::new();
    render_list(
        &ListMail {
            subject: t(locale, "alert-subject", &count),
            intro: t(locale, "alert-intro", &count),
            open_label: t(locale, "alert-open", &none),
            footer: t(locale, "alert-footer", &none),
            texts: &texts,
            lines: &lines,
            more: more_line(locale, notices.len() - shown),
        },
        app_url,
        locale,
    )
}

fn render_digest(
    comments: &[CommentsNotice],
    app_url: &Url,
    locale: Locale,
) -> Result<Rendered, MailError> {
    let shown = comments.len().min(MAX_LISTED_LINES);
    let (texts, lines): (Vec<String>, Vec<AlertLine>) = comments
        .iter()
        .take(shown)
        .map(|c| notice_line(&Notice::NewComments(c.clone()), locale))
        .unzip();
    let total: u64 = comments.iter().map(|c| u64::from(c.count)).sum();
    let count = Args::new().num("count", i64::try_from(total).unwrap_or(i64::MAX));
    let none = Args::new();
    render_list(
        &ListMail {
            subject: t(locale, "digest-subject", &count),
            intro: t(locale, "digest-intro", &count),
            open_label: t(locale, "digest-open", &none),
            footer: t(locale, "digest-footer", &none),
            texts: &texts,
            lines: &lines,
            more: more_line(locale, comments.len() - shown),
        },
        app_url,
        locale,
    )
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
