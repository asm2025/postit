//! Where an OAuth connection may send the browser back to: an exact-match allowlist built once
//! at startup, so `return_to` is never an open redirect.

use postit_config::Environment;
use url::Url;

/// The web app's landing path for a finished connection, relative to `app.public_url`.
const DONE_PATH: &str = "oauth-done";

/// The URLs `return_to` may name. A candidate matches only when it equals an entry exactly, as
/// a string: no prefix match, no normalization of the candidate.
#[derive(Debug, Clone)]
pub struct ReturnToAllowlist {
    /// `{app.public_url}/oauth-done`, where a callback without a trustworthy `return_to`
    /// lands.
    web: Url,
    /// Every allowed entry in its serialized form, `web` included.
    entries: Vec<String>,
}

impl ReturnToAllowlist {
    /// The allowlist of a deployment: `{app_public_url}/oauth-done`, the mobile app's
    /// `{scheme}://oauth-done` (`postit-dev`, `postit-qa`, or `postit` by environment), and
    /// each `extra` entry as `Url::parse` serializes it. An `extra` entry that does not parse
    /// is left out; the settings validation refuses one at startup.
    #[must_use]
    pub fn new(app_public_url: &Url, environment: Environment, extra: &[String]) -> Self {
        let web = done_url(app_public_url);
        let scheme = match environment {
            Environment::Development => "postit-dev",
            Environment::Qa => "postit-qa",
            Environment::Production => "postit",
        };
        let mut entries = vec![web.to_string()];
        // A custom scheme with a host and an empty path serializes as written.
        if let Ok(app) = Url::parse(&format!("{scheme}://{DONE_PATH}")) {
            entries.push(app.to_string());
        }
        entries.extend(
            extra
                .iter()
                .filter_map(|entry| Url::parse(entry).ok())
                .map(String::from),
        );
        entries.sort();
        entries.dedup();
        Self { web, entries }
    }

    /// Whether `candidate` equals an entry exactly.
    #[must_use]
    pub fn allows(&self, candidate: &str) -> bool {
        self.entries.iter().any(|entry| entry == candidate)
    }

    /// `{app.public_url}/oauth-done`.
    #[must_use]
    pub fn web(&self) -> &Url {
        &self.web
    }
}

/// `oauth-done` under `base`: one `/oauth-done` whether or not `base` ends in `/`.
fn done_url(base: &Url) -> Url {
    crate::engine::join_under(base, DONE_PATH)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn web_entry_has_one_slash_with_or_without_a_trailing_one() {
        for base in ["https://postit.local:44315", "https://postit.local:44315/"] {
            let list = ReturnToAllowlist::new(&Url::parse(base).unwrap(), Environment::Qa, &[]);
            assert_eq!(list.web().as_str(), "https://postit.local:44315/oauth-done");
            assert!(list.allows("postit-qa://oauth-done"));
        }
        let list = ReturnToAllowlist::new(
            &Url::parse("https://example.com/app").unwrap(),
            Environment::Production,
            &[],
        );
        assert_eq!(list.web().as_str(), "https://example.com/app/oauth-done");
        assert!(list.allows("postit://oauth-done"));
        assert!(!list.allows("postit-dev://oauth-done"));
    }
}
