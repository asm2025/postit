use std::sync::LazyLock;

use regex::Regex;
use url::Url;

pub const REDACTED: &str = "[redacted]";

/// Query parameter names that are never safe to log, redacted before a request's URL is
/// attached to its tracing span, and wherever [`redact_secrets`] finds them in text. Plan 03
/// phase B1 extends this from plugin-supplied patterns (for example signed media URLs); this
/// foundation list covers the common cases.
pub const SENSITIVE_QUERY_PARAMS: &[&str] = &["signature", "sig", "token", "access_token", "code"];

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
        // value led by an auth scheme (`Authorization: Basic …`) is redacted whole.
        (
            r#"(?i)\b([a-z0-9_.-]*(?:password|passwd|secret|token|api[_-]?key|apikey|key|authorization)[a-z0-9_.-]*)(["']?\s*[=:]\s*)(?:(?:bearer|basic|token)\s+)?("[^"]*"|'[^']*'|[^\s,;&"']+)"#.to_string(),
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

/// Returns `text` with credentials it may carry replaced by [`REDACTED`]: URL userinfo,
/// [`SENSITIVE_QUERY_PARAMS`] inside URLs, `Bearer` tokens, and `key=value` / `key: value`
/// pairs whose key looks secret (password, secret, token, api key, key, authorization). For
/// free-form error text that is stored or shown; it is a backstop, not a licence to put
/// secrets in messages.
#[must_use]
pub fn redact_secrets(text: &str) -> String {
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
