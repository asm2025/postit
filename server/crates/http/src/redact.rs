use url::Url;

pub const REDACTED: &str = "[redacted]";

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
}
