//! Pasted credentials: the token picks the result and `account_name` names the accounts.
//!
//! The token is compared after trimming and ASCII lowercasing, so a hand-typed `" Invalid "`
//! behaves as documented. Only the keyword it matched leaves this module; the token itself is
//! never logged, formatted, or copied into an error.

use std::time::Duration;

use postit_core::{
    AgentError, CredentialFields, Credentials, Discovered, DiscoveredAccount, OAuthTokens,
};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::script::VerifyScript;

/// How long the `rate_limited` token asks the caller to wait.
pub const RATE_LIMIT_DELAY: Duration = Duration::from_secs(30);

/// The prefix of every mock account's remote ID.
pub const ACCOUNT_PREFIX: &str = "mock-account-";

/// What a pasted token asks the mock to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Token {
    /// `invalid`: verification fails on the `token` field.
    Invalid,
    /// `rate_limited`: verification is rate limited.
    RateLimited,
    /// `expired`: connects, but later calls report the account needs reconnecting.
    Expired,
    /// Anything else.
    Valid,
}

impl Token {
    pub(crate) fn of(token: &SecretString) -> Self {
        match token.expose_secret().trim().to_ascii_lowercase().as_str() {
            "invalid" => Self::Invalid,
            "rate_limited" => Self::RateLimited,
            "expired" => Self::Expired,
            _ => Self::Valid,
        }
    }

    /// The token stored for a grant. OAuth tokens are always valid; the mock OAuth provider
    /// scripts its own failures.
    pub(crate) fn of_grant(credentials: &Credentials) -> Self {
        match credentials {
            Credentials::OAuth(OAuthTokens { .. }) => Self::Valid,
            Credentials::Fields(fields) => {
                fields.get_secret("token").map_or(Self::Invalid, Self::of)
            }
        }
    }

    /// Whether the platform rejects calls made with this token.
    pub(crate) fn rejected(self) -> bool {
        matches!(self, Self::Invalid | Self::Expired)
    }
}

/// The names in `account_name`: split on commas, trimmed, empties dropped, de-duplicated in
/// first-seen order.
pub(crate) fn account_names(fields: &CredentialFields) -> Vec<String> {
    let raw = fields
        .get_plain("account_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for name in raw.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if !names.iter().any(|seen| seen == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// The account a name stands for.
pub(crate) fn account(name: &str) -> DiscoveredAccount {
    DiscoveredAccount {
        remote_id: format!("{ACCOUNT_PREFIX}{name}"),
        display_name: name.to_owned(),
        handle: Some(format!("@{name}")),
        avatar_url: None,
    }
}

/// The accounts the pasted fields reach, or why the mock platform refuses them.
pub(crate) fn discover(fields: &CredentialFields) -> Result<Discovered, AgentError> {
    let token = fields.get_secret("token").ok_or_else(|| invalid("token"))?;
    match Token::of(token) {
        Token::Invalid => return Err(invalid("token")),
        Token::RateLimited => {
            return Err(AgentError::RateLimited {
                retry_after: Some(RATE_LIMIT_DELAY),
            });
        }
        Token::Expired | Token::Valid => {}
    }
    let names = account_names(fields);
    if names.is_empty() {
        return Err(invalid("account_name"));
    }
    Ok(Discovered {
        accounts: names.iter().map(|n| account(n)).collect(),
    })
}

/// Runs a [`VerifyScript`] override against the pasted fields.
pub(crate) async fn scripted(
    mut script: VerifyScript,
    fields: &CredentialFields,
) -> Result<Discovered, AgentError> {
    loop {
        match script {
            VerifyScript::Accounts(accounts) => return Ok(Discovered { accounts }),
            VerifyScript::Invalid(field) => return Err(AgentError::InvalidCredentials { field }),
            VerifyScript::RateLimited => return Err(AgentError::RateLimited { retry_after: None }),
            VerifyScript::Fail => {
                return Err(AgentError::Remote {
                    status: Some(500),
                    code: None,
                    message: "the mock platform failed".to_owned(),
                });
            }
            VerifyScript::ByFields(decide) => {
                let (delay, next) = decide(fields);
                tokio::time::sleep(delay).await;
                script = next;
            }
        }
    }
}

fn invalid(field: &str) -> AgentError {
    AgentError::InvalidCredentials {
        field: Some(field.to_owned()),
    }
}

/// The error a call made with a rejected token gets.
pub(crate) fn rejected_token() -> AgentError {
    AgentError::Authentication {
        message: "the mock platform rejected the token".to_owned(),
    }
}
