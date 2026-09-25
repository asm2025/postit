use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::environment::Environment;
use crate::error::ConfigError;
use crate::secret::RedactedSecret;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsSettings {
    pub enabled: bool,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    pub enabled: bool,
    pub root: Option<PathBuf>,
    pub bind: Option<String>,
    pub api_base_url: Option<Url>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSettings {
    pub host: String,
    pub api_port: u16,
    pub worker_port: u16,
    pub tls: TlsSettings,
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
    pub public_url: Url,
    #[serde(with = "crate::duration")]
    pub shutdown_timeout: Duration,
    pub web: WebSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    pub url: RedactedSecret,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserinfoMode {
    Fallback,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcClaimNames {
    pub email: String,
    pub email_verified: String,
    pub name: String,
    pub preferred_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcSettings {
    pub issuer: Url,
    pub audiences: Vec<String>,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub accepted_algorithms: Vec<String>,
    #[serde(with = "crate::duration")]
    pub leeway: Duration,
    #[serde(with = "crate::duration")]
    pub jwks_refresh_interval: Duration,
    pub claim_names: OidcClaimNames,
    pub userinfo: UserinfoMode,
    pub account_url: Option<Url>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSettings {
    pub admin_email: Option<String>,
    pub admin_subject: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSettings {
    pub oidc: OidcSettings,
    pub bootstrap: BootstrapSettings,
    #[serde(with = "crate::duration")]
    pub pending_ttl: Duration,
    #[serde(with = "crate::duration")]
    pub principal_cache_ttl: Duration,
    #[serde(with = "crate::duration")]
    pub approval_email_interval: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditSettings {
    #[serde(with = "crate::duration")]
    pub retention: Duration,
    #[serde(with = "crate::duration")]
    pub ip_retention: Duration,
    pub pseudonym_key: Option<RedactedSecret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionSettings {
    #[serde(with = "crate::duration")]
    pub notifications_read_after: Duration,
    #[serde(with = "crate::duration")]
    pub delivery_attempts_after: Duration,
    #[serde(with = "crate::duration")]
    pub ai_generation_prompt_after: Duration,
    #[serde(with = "crate::duration")]
    pub ai_generation_row_after: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsSettings {
    #[serde(with = "crate::duration")]
    pub check_interval: Duration,
    #[serde(with = "crate::duration")]
    pub due_delivery_overdue_after: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateBucket {
    pub rate_per_minute: u32,
    pub burst: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitSettings {
    pub unauthenticated: RateBucket,
    pub provisioning: RateBucket,
    pub authenticated: RateBucket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailTransport {
    Smtp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<RedactedSecret>,
    pub starttls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailSettings {
    pub transport: MailTransport,
    pub from_address: String,
    pub smtp: SmtpSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobHistoryRetention {
    #[serde(with = "crate::duration")]
    pub succeeded: Duration,
    #[serde(with = "crate::duration")]
    pub failed: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobsSettings {
    #[serde(default)]
    pub concurrency: HashMap<String, u32>,
    #[serde(with = "crate::duration")]
    pub outbox_poll_interval: Duration,
    pub history_retention: JobHistoryRetention,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpSettings {
    #[serde(with = "crate::duration")]
    pub connect_timeout: Duration,
    #[serde(with = "crate::duration")]
    pub request_timeout: Duration,
    pub user_agent: String,
    #[serde(default)]
    pub extra_ca_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    pub public_url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorsSettings {
    pub allowed_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub server: ServerSettings,
    pub database: DatabaseSettings,
    pub auth: AuthSettings,
    pub audit: AuditSettings,
    pub retention: RetentionSettings,
    pub ops: OpsSettings,
    pub rate_limit: RateLimitSettings,
    pub mail: MailSettings,
    pub jobs: JobsSettings,
    pub http: HttpSettings,
    pub app: AppSettings,
    pub cors: CorsSettings,
}

impl Settings {
    /// Validation rules that layering and serde alone can't express.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Validation`] when the OIDC issuer isn't `https` outside
    /// development, `auth.oidc.audiences` is empty, both bootstrap fields are set, or
    /// `audit.pseudonym_key` is missing outside development.
    pub fn validate(&self, env: Environment) -> Result<(), ConfigError> {
        if env != Environment::Development && self.auth.oidc.issuer.scheme() != "https" {
            return Err(ConfigError::Validation(
                "auth.oidc.issuer must be https outside development".into(),
            ));
        }

        if self.auth.oidc.audiences.is_empty() {
            return Err(ConfigError::Validation(
                "auth.oidc.audiences must not be empty".into(),
            ));
        }

        if self.auth.bootstrap.admin_email.is_some() && self.auth.bootstrap.admin_subject.is_some()
        {
            return Err(ConfigError::Validation(
                "auth.bootstrap.admin_email and admin_subject are mutually exclusive".into(),
            ));
        }

        if env != Environment::Development && self.audit.pseudonym_key.is_none() {
            return Err(ConfigError::Validation(
                "audit.pseudonym_key is required outside development".into(),
            ));
        }

        Ok(())
    }

    /// A `serde_json::Value` safe to log or return from an admin endpoint: every
    /// `RedactedSecret` field serializes as `"[redacted]"`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Load`] if serialization itself fails, which does not happen
    /// for a `Settings` value obtained through [`crate::load`].
    pub fn redacted_dump(&self) -> Result<serde_json::Value, ConfigError> {
        serde_json::to_value(self).map_err(|err| ConfigError::Load(err.to_string()))
    }
}
