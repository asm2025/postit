use std::sync::{LazyLock, OnceLock};

use regex::Regex;
use url::Url;

/// Replacement text for every redacted value.
pub const REDACTED: &str = "[redacted]";

/// What an opaque URL (see [`Sensitivity::Opaque`]) is logged as.
pub const REDACTED_URL: &str = "[redacted-url]";

/// Query parameter names that are never safe to log, redacted before a request's URL is
/// attached to its tracing span, and wherever [`redact_secrets`] finds them in text. URLs
/// whose secret lives in the path are covered by [`SensitiveUrl`] patterns instead.
pub const SENSITIVE_QUERY_PARAMS: &[&str] = &[
    "signature",
    "sig",
    "token",
    "access_token",
    "code",
    "client_secret",
    "refresh_token",
    "id_token",
    "exp",
    // OAuth `state` is a CSRF token.
    "state",
];

/// How much of a request's URL may be logged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sensitivity {
    /// Logged through the [`Redactor`]: sensitive URL patterns and query parameters
    /// redacted, the rest kept.
    #[default]
    Normal,
    /// The whole URL is secret (a web push endpoint): logged as [`REDACTED_URL`].
    Opaque,
}

/// A URL family whose path carries a secret (a webhook token, a signed media id). A URL
/// matching it is logged as `scheme://host/[redacted]`, with no path or query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SensitiveUrl {
    /// The host, matched ASCII case-insensitively and ignoring a trailing dot.
    pub host: String,
    /// When set, subdomains of `host` match too.
    pub host_suffix: bool,
    /// When set, only paths starting with this prefix match. A `{n}` in it stands for one or
    /// more ASCII digits (an API version: `/api/v{n}/webhooks`).
    pub path_prefix: Option<String>,
}

impl SensitiveUrl {
    fn matches(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        host_matches(host, &self.host, self.host_suffix)
            && self
                .path_prefix
                .as_deref()
                .is_none_or(|prefix| path_has_prefix(url.path(), prefix))
    }
}

/// Whether `path` starts with `prefix`, where each `{n}` in `prefix` matches one or more
/// ASCII digits.
fn path_has_prefix(path: &str, prefix: &str) -> bool {
    let mut parts = prefix.split("{n}");
    let Some(first) = parts.next() else {
        return true;
    };
    let Some(mut rest) = path.strip_prefix(first) else {
        return false;
    };
    for literal in parts {
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            return false;
        }
        match rest
            .get(digits..)
            .and_then(|tail| tail.strip_prefix(literal))
        {
            Some(tail) => rest = tail,
            None => return false,
        }
    }
    true
}

/// `host` equals `pattern` (or, with `suffix`, is a subdomain of it), ASCII
/// case-insensitively and ignoring a trailing dot on either.
fn host_matches(host: &str, pattern: &str, suffix: bool) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    let pattern = pattern.strip_suffix('.').unwrap_or(pattern);
    if pattern.is_empty() {
        return false;
    }
    if host.eq_ignore_ascii_case(pattern) {
        return true;
    }
    suffix
        && host.len() > pattern.len()
        && host
            .len()
            .checked_sub(pattern.len() + 1)
            .and_then(|dot| host.get(dot..))
            .and_then(|tail| tail.strip_prefix('.'))
            .is_some_and(|tail| tail.eq_ignore_ascii_case(pattern))
}

/// The static redaction rules plus registered [`SensitiveUrl`] patterns. One is installed
/// process-wide at startup with [`install_redactor`]; tests that need custom patterns build
/// a local one.
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    urls: Vec<SensitiveUrl>,
}

impl Redactor {
    /// The built-in patterns: Slack incoming webhooks (`hooks.slack.com`) and Discord
    /// webhooks (`discord.com` and `discordapp.com` with their subdomains such as `canary.`
    /// and `ptb.`, under `/api/webhooks` or a versioned `/api/v{n}/webhooks`).
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut redactor = Self::default();
        redactor.add(SensitiveUrl {
            host: "hooks.slack.com".to_string(),
            host_suffix: false,
            path_prefix: None,
        });
        for host in ["discord.com", "discordapp.com"] {
            for prefix in ["/api/webhooks", "/api/v{n}/webhooks"] {
                redactor.add(SensitiveUrl {
                    host: host.to_string(),
                    host_suffix: true,
                    path_prefix: Some(prefix.to_string()),
                });
            }
        }
        redactor
    }

    /// Registers another sensitive URL pattern.
    pub fn add(&mut self, url: SensitiveUrl) {
        self.urls.push(url);
    }

    fn is_sensitive(&self, url: &Url) -> bool {
        self.urls.iter().any(|pattern| pattern.matches(url))
    }

    /// `url` safe to log: `scheme://host/[redacted]` when it matches a registered pattern,
    /// otherwise the URL without userinfo and with [`SENSITIVE_QUERY_PARAMS`] redacted.
    #[must_use]
    pub fn redact_url(&self, url: &Url) -> String {
        if self.is_sensitive(url) {
            let host = url.host_str().unwrap_or_default();
            return format!("{}://{host}/{REDACTED}", url.scheme());
        }
        let mut safe = redact_query_params(url, SENSITIVE_QUERY_PARAMS);
        // Ignoring the setter results is fine: they fail only for cannot-be-a-base URLs,
        // which carry no userinfo to begin with.
        let _ = safe.set_username("");
        let _ = safe.set_password(None);
        safe.into()
    }

    /// `url` rendered for a span, a log line, or error text under `sensitivity`.
    #[must_use]
    pub fn render_url(&self, url: &Url, sensitivity: Sensitivity) -> String {
        match sensitivity {
            Sensitivity::Normal => self.redact_url(url),
            Sensitivity::Opaque => REDACTED_URL.to_string(),
        }
    }

    /// [`redact_secrets`] with this redactor's patterns: every `http(s)` URL in `text` that
    /// matches a registered pattern is replaced first, then the static rules run.
    #[must_use]
    pub fn redact_text(&self, text: &str) -> String {
        static_redact(&self.redact_urls_in(text))
    }

    fn redact_urls_in(&self, text: &str) -> String {
        let Some(re) = URL_IN_TEXT.as_ref() else {
            return REDACTED.to_string();
        };
        re.replace_all(text, |caps: &regex::Captures<'_>| {
            let raw = caps.get(0).map_or("", |m| m.as_str());
            // Punctuation closing a sentence or a parenthesis is not part of the URL.
            let url_part = raw.trim_end_matches([')', ']', '}', ',', ';', '.', ':', '!', '?']);
            let tail = raw.get(url_part.len()..).unwrap_or_default();
            match Url::parse(url_part) {
                Ok(url) if self.is_sensitive(&url) => {
                    format!("{}{tail}", self.redact_url(&url))
                }
                _ => raw.to_string(),
            }
        })
        .into_owned()
    }
}

/// An `http(s)` URL inside free text. `None` if it failed to compile, in which case the
/// text is redacted whole.
static URL_IN_TEXT: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(?i)\bhttps?://[^\s"'<>]+"#).ok());

static INSTALLED: OnceLock<Redactor> = OnceLock::new();
static DEFAULT_REDACTOR: LazyLock<Redactor> = LazyLock::new(Redactor::with_defaults);

/// Sets the process-wide redactor. Call once at startup; returns `false` (and changes
/// nothing) if one was already installed.
pub fn install_redactor(redactor: Redactor) -> bool {
    INSTALLED.set(redactor).is_ok()
}

/// The process-wide redactor: the installed one, or [`Redactor::with_defaults`] when none
/// was installed.
#[must_use]
pub fn redactor() -> &'static Redactor {
    INSTALLED.get().unwrap_or_else(|| &DEFAULT_REDACTOR)
}

/// `(pattern, replacement)` pairs applied in order by [`redact_secrets`]. `None` if a pattern
/// failed to compile, in which case the whole text is redacted.
static SECRET_PATTERNS: LazyLock<Option<Vec<(Regex, &'static str)>>> = LazyLock::new(|| {
    let params = SENSITIVE_QUERY_PARAMS
        .iter()
        .map(|p| regex::escape(p))
        .collect::<Vec<_>>()
        .join("|");
    [
        // URL userinfo: `scheme://user:pass@host`.
        (
            r"(?i)\b([a-z][a-z0-9+.-]*://)[^/?#\s@]+@".to_string(),
            "${1}[redacted]@",
        ),
        // `key=value` / `key: value` (also JSON `"key": "value"`) whose key looks secret; a
        // value led by an auth scheme (`Authorization: Basic …`) is redacted whole. A quoted
        // value with no closing quote (text cut mid-value) is redacted to the end.
        (
            r#"(?i)\b([a-z0-9_.-]*(?:password|passwd|secret|token|api[_-]?key|apikey|key|authorization)[a-z0-9_.-]*)(["']?\s*[=:]\s*)(?:(?:bearer|basic|token)\s+)?("[^"]*"?|'[^']*'?|[^\s,;&"']+)"#.to_string(),
            "${1}${2}[redacted]",
        ),
        // Token-bearing query parameters inside a URL.
        (format!(r"([?&](?:{params})=)[^&#\s]*"), "${1}[redacted]"),
        // `Bearer <token>` (RFC 6750 b64token).
        (
            r"(?i)\b(bearer)\s+[a-z0-9._~+/-]+=*".to_string(),
            "${1} [redacted]",
        ),
        // Email addresses: personal data that free-form error text must not carry.
        (
            r"(?i)[a-z0-9._%+-]+@[a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,}".to_string(),
            "[redacted-email]",
        ),
    ]
    .into_iter()
    .map(|(pattern, replacement)| Regex::new(&pattern).ok().map(|re| (re, replacement)))
    .collect()
});

/// Returns `text` with credentials it may carry replaced by [`REDACTED`]: URLs matching the
/// process-wide [`redactor`]'s patterns, URL userinfo,
/// [`SENSITIVE_QUERY_PARAMS`] inside URLs, `Bearer` tokens, and `key=value` / `key: value`
/// pairs whose key looks secret (password, secret, token, api key, key, authorization). For
/// free-form error text that is stored or shown; it is a backstop, not a licence to put
/// secrets in messages.
#[must_use]
pub fn redact_secrets(text: &str) -> String {
    redactor().redact_text(text)
}

/// The static rules alone, without URL patterns.
fn static_redact(text: &str) -> String {
    let Some(patterns) = SECRET_PATTERNS.as_ref() else {
        return REDACTED.to_string();
    };
    patterns
        .iter()
        .fold(text.to_string(), |acc, (re, replacement)| {
            re.replace_all(&acc, *replacement).into_owned()
        })
}

/// A header value safe to log, regardless of what it actually holds. Call this instead of
/// logging `Authorization` (or any other token-bearing header) directly.
#[must_use]
pub fn redact_header_value(_value: &str) -> &'static str {
    REDACTED
}

/// Returns a copy of `url` with every query parameter named in `sensitive_params` replaced
/// by [`REDACTED`], so the rest of the URL (host, path) stays useful in logs.
#[must_use]
pub fn redact_query_params(url: &Url, sensitive_params: &[&str]) -> Url {
    let mut redacted = url.clone();
    let pairs: Vec<(String, String)> = redacted
        .query_pairs()
        .map(|(key, value)| {
            if sensitive_params.contains(&key.as_ref()) {
                (key.into_owned(), REDACTED.to_string())
            } else {
                (key.into_owned(), value.into_owned())
            }
        })
        .collect();

    if pairs.is_empty() {
        redacted.set_query(None);
    } else {
        redacted
            .query_pairs_mut()
            .clear()
            .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }

    redacted
}

/// `err`'s message with its URL (which reqwest appends to the text) rendered through the
/// process-wide [`redactor`]: a sensitive URL loses its path, other URLs lose userinfo and
/// sensitive query parameters. Use this instead of formatting a `reqwest::Error` directly.
#[must_use]
pub fn redacted_error_text(err: &reqwest::Error) -> String {
    let text = err.to_string();
    let Some(url) = err.url() else {
        return redact_secrets(&text);
    };
    redact_secrets(&text.replace(url.as_str(), &redactor().redact_url(url)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn must_parse(raw: &str) -> Url {
        Url::parse(raw).unwrap_or_else(|_| unreachable!("hardcoded valid url literal: {raw}"))
    }

    #[test]
    fn redacts_named_query_params_only() {
        let url = must_parse("https://api.example.com/path?token=secret&page=2");
        let redacted = redact_query_params(&url, &["token"]);

        assert!(redacted.as_str().contains("token=%5Bredacted%5D"));
        assert!(redacted.as_str().contains("page=2"));
        assert!(!redacted.as_str().contains("secret"));
    }

    #[test]
    fn redacts_url_userinfo() {
        assert_eq!(
            redact_secrets("connect to postgres://app:hunter2@db:5432/postit failed"),
            "connect to postgres://[redacted]@db:5432/postit failed"
        );
    }

    #[test]
    fn redacts_sensitive_query_params_in_text() {
        assert_eq!(
            redact_secrets("GET https://h.example/cb?code=abc&page=2&sig=x9 returned 500"),
            "GET https://h.example/cb?code=[redacted]&page=2&sig=[redacted] returned 500"
        );
    }

    #[test]
    fn redacts_bearer_tokens() {
        assert_eq!(
            redact_secrets("sent Bearer eyJhbGciOi.payload.sig-_+/= upstream"),
            "sent Bearer [redacted] upstream"
        );
    }

    #[test]
    fn redacts_secret_looking_key_value_pairs() {
        assert_eq!(
            redact_secrets(
                "password=hunter2, api_key: k-123; client_secret=\"a b\" Authorization: Basic dXNlcg== retries=3"
            ),
            "password=[redacted], api_key: [redacted]; client_secret=[redacted] Authorization: [redacted] retries=3"
        );
        assert_eq!(
            redact_secrets(r#"{"access_token": "t0k", "status": "ok"}"#),
            r#"{"access_token": [redacted], "status": "ok"}"#
        );
    }

    #[test]
    fn redacts_a_quoted_value_cut_before_its_closing_quote() {
        assert_eq!(
            redact_secrets(r#"{"access_token": "ya29.SECRETPREFIX"#),
            r#"{"access_token": [redacted]"#
        );
        assert_eq!(
            redact_secrets("password='hunter2 still going"),
            "password=[redacted]"
        );
    }

    #[test]
    fn redacts_email_addresses() {
        assert_eq!(
            redact_secrets(
                "rejected Ada.Lovelace+jobs@mail.example.co.uk: 550, cc bob@example.com"
            ),
            "rejected [redacted-email]: 550, cc [redacted-email]"
        );
        // Userinfo is masked first and leaves no address behind.
        assert_eq!(
            redact_secrets("smtp://mailer:pw@smtp.example.com down"),
            "smtp://[redacted]@smtp.example.com down"
        );
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let text =
            "relation \"jobs\" does not exist; status code: 500 at https://h.example/a?page=2";
        assert_eq!(redact_secrets(text), text);
    }
}
