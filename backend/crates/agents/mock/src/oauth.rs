//! The OAuth hooks of `mock_oauth`: the scripted code exchange and the signed `deauthorize`
//! event.
//!
//! A real OAuth agent verifies what the platform signed with a constant-time comparison, as
//! [`deauthorize`] does.

use std::collections::BTreeMap;

use hmac::{Hmac, Mac};
use postit_core::{
    AccountToken, AgentError, EventContext, EventOutcome, EventResponse, GrantExchange,
    InboundEvent, TokenExchange,
};
use secrecy::SecretString;
use serde_json::Value;
use sha2::Sha256;

use crate::script::ExchangeScript;

/// The header carrying the lowercase hex HMAC-SHA256 of the raw body.
pub const SIGNATURE_HEADER: &str = "X-Mock-Signature";

/// The event kind a deauthorization arrives as.
pub const DEAUTHORIZE: &str = "deauthorize";

/// Applies `script` to a finished exchange.
pub(crate) fn exchange(
    script: &ExchangeScript,
    exchange: TokenExchange,
) -> Result<GrantExchange, AgentError> {
    if script.fail {
        return Err(AgentError::Authentication {
            message: "the mock platform refused the exchange".to_owned(),
        });
    }
    let account_tokens = script
        .account_tokens
        .iter()
        .map(|id| {
            (
                id.clone(),
                AccountToken {
                    access_token: SecretString::from(format!("acct-token-{id}")),
                    expires_at: None,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    Ok(GrantExchange {
        tokens: exchange.tokens,
        remote_user_id: script.remote_user_id.clone(),
        reconnect_by: script.reconnect_by,
        account_tokens,
    })
}

fn bad_signature() -> AgentError {
    AgentError::Authentication {
        message: "the event signature is missing or wrong".to_owned(),
    }
}

fn bad_body(message: &str) -> AgentError {
    AgentError::Remote {
        status: Some(400),
        code: None,
        message: message.to_owned(),
    }
}

/// Handles a `deauthorize` event: the body is `{"user_id": "<remote>"}` and the signature
/// header the lowercase hex HMAC-SHA256 of the raw body keyed with the configured
/// `client_secret`, exactly: uppercase digits or surrounding whitespace are refused.
pub(crate) fn deauthorize(
    ctx: &EventContext,
    event: &InboundEvent,
) -> Result<EventOutcome, AgentError> {
    let secret = ctx
        .config
        .get("client_secret")
        .and_then(Value::as_str)
        .ok_or_else(bad_signature)?;
    let signature = event
        .header(SIGNATURE_HEADER)
        .and_then(|raw| std::str::from_utf8(raw).ok())
        .filter(|text| text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .and_then(|text| hex::decode(text).ok())
        .ok_or_else(bad_signature)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).map_err(|_| bad_signature())?;
    mac.update(&event.raw_body);
    mac.verify_slice(&signature).map_err(|_| bad_signature())?;
    let body: Value = serde_json::from_slice(&event.raw_body)
        .map_err(|_| bad_body("the deauthorize body is not JSON"))?;
    let user = body
        .get("user_id")
        .and_then(Value::as_str)
        .ok_or_else(|| bad_body("the deauthorize body has no user_id"))?;
    Ok(EventOutcome {
        response: EventResponse::empty(200),
        deauthorized: vec![user.to_owned()],
    })
}
