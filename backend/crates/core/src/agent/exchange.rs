//! What an agent can add to a finished OAuth code exchange.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde_json::{Map, Value};

use super::context::{HttpHandles, OAuthTokens};
use crate::Clock;

/// What `after_exchange` gets: no stored grant exists yet.
#[derive(Clone)]
pub struct ExchangeContext {
    /// The agent's stored configuration.
    pub config: Value,
    /// HTTP clients.
    pub http: HttpHandles,
    /// The time source.
    pub clock: Arc<dyn Clock>,
}

/// The token endpoint's JSON response, exactly as received. It may carry secrets and
/// platform-specific fields, so `Debug` prints nothing of it.
#[derive(Clone)]
pub struct RawTokenResponse(Map<String, Value>);

impl RawTokenResponse {
    /// Wraps a parsed response body.
    #[must_use]
    pub const fn new(map: Map<String, Value>) -> Self {
        Self(map)
    }

    /// The field named `name`, when the response has it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }
}

impl fmt::Debug for RawTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RawTokenResponse([redacted])")
    }
}

/// A finished code exchange, as the engine parsed it.
#[derive(Debug, Clone)]
pub struct TokenExchange {
    /// The standard fields, parsed.
    pub tokens: OAuthTokens,
    /// The whole response, for platform-specific fields.
    pub raw: RawTokenResponse,
}

/// A token for one account behind a grant (a Facebook page token, for example).
#[derive(Clone)]
pub struct AccountToken {
    /// The bearer token.
    pub access_token: SecretString,
    /// When it expires, when the platform says.
    pub expires_at: Option<DateTime<Utc>>,
}

impl fmt::Debug for AccountToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountToken")
            .field("access_token", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// What the engine stores for a new OAuth grant, after the agent has had its say.
#[derive(Debug, Clone)]
pub struct GrantExchange {
    /// The grant's tokens, possibly replaced (a long-lived token exchanged for the short one).
    pub tokens: OAuthTokens,
    /// The platform's ID for the user who approved, when the response names one.
    pub remote_user_id: Option<String>,
    /// When the user must reconnect regardless of refreshing, when the platform says.
    pub reconnect_by: Option<DateTime<Utc>>,
    /// Per-account tokens, keyed by the account's remote ID.
    pub account_tokens: BTreeMap<String, AccountToken>,
}

impl GrantExchange {
    /// A result that keeps the tokens as exchanged and adds nothing.
    #[must_use]
    pub const fn from_tokens(tokens: OAuthTokens) -> Self {
        Self {
            tokens,
            remote_user_id: None,
            reconnect_by: None,
            account_tokens: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{TimeZone, Utc};
    use secrecy::SecretString;
    use serde_json::json;

    use super::*;
    use crate::agent::test_support::{FakeAgent, fake_clock, fake_http};
    use crate::{Agent, OAuthTokens};

    fn tokens() -> OAuthTokens {
        OAuthTokens {
            access_token: SecretString::from("tok-1".to_owned()),
            refresh_token: Some(SecretString::from("tok-2".to_owned())),
            expires_at: Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()),
            scopes: vec!["read".to_owned()],
        }
    }

    fn raw() -> RawTokenResponse {
        let mut map = serde_json::Map::new();
        map.insert("access_token".to_owned(), json!("tok-1"));
        RawTokenResponse::new(map)
    }

    #[tokio::test]
    async fn after_exchange_default_passes_tokens_through() {
        let agent = FakeAgent::new("demo");
        let ctx = ExchangeContext {
            config: serde_json::Value::Null,
            http: fake_http(),
            clock: fake_clock(),
        };
        let input = tokens();
        let expires = input.expires_at;
        let exchange = TokenExchange {
            tokens: input,
            raw: raw(),
        };
        let grant = agent.after_exchange(&ctx, exchange).await.unwrap();
        assert_eq!(grant.tokens.expires_at, expires);
        assert!(grant.remote_user_id.is_none());
        assert!(grant.reconnect_by.is_none());
        assert!(grant.account_tokens.is_empty());
    }

    #[test]
    fn debug_redacts_exchange_types() {
        let raw = raw();
        assert!(raw.get("access_token").is_some());
        assert!(!format!("{raw:?}").contains("tok-1"));
        let account = AccountToken {
            access_token: SecretString::from("tok-1".to_owned()),
            expires_at: None,
        };
        assert!(!format!("{account:?}").contains("tok-1"));
        let mut grant = GrantExchange::from_tokens(tokens());
        grant.account_tokens.insert("a".to_owned(), account);
        let shown = format!("{grant:?}");
        assert!(!shown.contains("tok-1"), "{shown}");
        assert!(!shown.contains("tok-2"), "{shown}");
    }
}
