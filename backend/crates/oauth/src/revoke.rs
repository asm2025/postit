//! Revoking tokens at the provider: the one primitive every OAuth revoke goes through.

use std::sync::Arc;

use postit_core::{ConnectMethod, CredentialStore, GrantExchange, OAuthTokens, PlatformId};
use serde_json::Value;

use crate::AccountService;
use crate::engine::{self, ClientCredentials};
use crate::error::agent_error_name;
use crate::flow::ExchangedCredentials;

impl AccountService {
    /// Best effort: asks the provider to revoke `tokens`. When the spec has an RFC 7009
    /// endpoint the engine sends the refresh token (hint `refresh_token`), or the access token
    /// when there is none (hint `access_token`); otherwise the agent's `revoke()` runs over an
    /// in-memory store holding `tokens`. A failure is logged by its kind only. Whether the
    /// revoke should happen at all (the revoke-skip rule) is the caller's call.
    pub(crate) async fn revoke_tokens(
        &self,
        platform: &PlatformId,
        config: &Value,
        tokens: &OAuthTokens,
    ) {
        let Some(agent) = self.deps.catalog.registry().get(platform) else {
            tracing::warn!(%platform, "no agent to revoke the tokens with");
            return;
        };
        let ConnectMethod::OAuth(spec) = &agent.descriptor().connect else {
            tracing::warn!(%platform, "the platform does not connect through OAuth; nothing revoked");
            return;
        };
        if spec.revoke_url.is_some() {
            let Ok(client) = ClientCredentials::from_config(config) else {
                tracing::warn!(%platform, "no client credentials to revoke the tokens with");
                return;
            };
            let (token, hint) = match &tokens.refresh_token {
                Some(refresh) => (refresh, "refresh_token"),
                None => (&tokens.access_token, "access_token"),
            };
            if let Err(error) = engine::revoke(&self.deps.http, spec, &client, token, hint).await {
                tracing::warn!(%platform, %error, "revoking the tokens did not succeed");
            }
            return;
        }
        let store: Arc<dyn CredentialStore> = Arc::new(ExchangedCredentials::new(
            &GrantExchange::from_tokens(tokens.clone()),
        ));
        let context = self.agent_context_with(config.clone(), store);
        match agent.create_client(context) {
            Ok(client) => {
                if let Err(error) = client.revoke().await {
                    tracing::warn!(%platform, error = agent_error_name(&error), "revoking the tokens did not succeed");
                }
            }
            Err(error) => {
                tracing::warn!(%platform, error = agent_error_name(&error), "no client to revoke the tokens with");
            }
        }
    }
}
