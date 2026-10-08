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
    #[serde(with = "crate::duration", default = "default_request_timeout")]
    pub request_timeout: Duration,
    #[serde(default = "default_body_limit")]
    pub body_limit: usize,
    pub web: WebSettings,
}

fn default_request_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_body_limit() -> usize {
    1_048_576
}

const MAX_DURATION: Duration = Duration::from_hours(100 * 365 * 24);

fn check_duration(key: &str, value: Duration, allow_zero: bool) -> Result<(), ConfigError> {
    if (!allow_zero && value.is_zero()) || value > MAX_DURATION {
        let lower = if allow_zero {
            "at least 0"
        } else {
            "greater than 0"
        };
        return Err(ConfigError::Validation(format!(
            "{key} must be {lower} and at most 100 years"
        )));
    }
    Ok(())
}

fn check_bucket(key: &str, bucket: &RateBucket) -> Result<(), ConfigError> {
    if bucket.rate_per_minute == 0 || bucket.burst == 0 {
        return Err(ConfigError::Validation(format!(
            "{key}.rate_per_minute and {key}.burst must both be at least 1"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    /// Where the database is — `postgres://host:port/name`, never credentials, which
    /// are secrets and come from `username` / `password`.
    pub url: Url,
    pub username: String,
    pub password: RedactedSecret,
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
    pub pseudonym_key: RedactedSecret,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpTls {
    /// Plain SMTP, no TLS. Development only (a local SMTP tool such as Papercut).
    None,
    Starttls,
    /// Implicit TLS (usually port 465).
    Tls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<RedactedSecret>,
    pub tls: SmtpTls,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailSettings {
    pub transport: MailTransport,
    pub from_address: String,
    pub send_email_max_attempts: u32,
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
pub struct JobSchedules {
    pub job_history_purge: String,
    pub purge_pending_users: String,
    pub audit_retention: String,
    pub data_retention: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobsSettings {
    #[serde(default)]
    pub concurrency: HashMap<String, u32>,
    #[serde(with = "crate::duration")]
    pub outbox_poll_interval: Duration,
    pub history_retention: JobHistoryRetention,
    pub schedules: JobSchedules,
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
    /// development, `auth.oidc.audiences` is empty, both bootstrap fields are set,
    /// `database.url` carries a username or password, or `mail.smtp.tls` is `none` outside
    /// development, a duration is zero (except `auth.oidc.leeway`) or over 100 years, a
    /// rate-limit bucket has a zero rate or burst, `server.body_limit` is zero,
    /// `server.web.enabled` has no `server.web.root`, or `server.web.bind` is not a socket address.
    pub fn validate(&self, env: Environment) -> Result<(), ConfigError> {
        let db_url = &self.database.url;
        if !db_url.username().is_empty() || db_url.password().is_some() {
            return Err(ConfigError::Validation(
                "database.url must not carry credentials; set database.username and \
                 database.password (secrets) instead"
                    .into(),
            ));
        }

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

        if env != Environment::Development && self.mail.smtp.tls == SmtpTls::None {
            return Err(ConfigError::Validation(
                "mail.smtp.tls = \"none\" is only allowed in development".into(),
            ));
        }

        let web = &self.server.web;
        if web.enabled && web.root.is_none() {
            return Err(ConfigError::Validation(
                "server.web.enabled requires server.web.root".into(),
            ));
        }
        if let Some(bind) = &web.bind
            && bind.parse::<std::net::SocketAddr>().is_err()
        {
            return Err(ConfigError::Validation(
                "server.web.bind must be an IP socket address such as 0.0.0.0:44315".into(),
            ));
        }

        self.validate_bounds()
    }

    fn validate_bounds(&self) -> Result<(), ConfigError> {
        let s = self;
        for (key, value) in [
            ("server.shutdown_timeout", s.server.shutdown_timeout),
            ("server.request_timeout", s.server.request_timeout),
            (
                "auth.oidc.jwks_refresh_interval",
                s.auth.oidc.jwks_refresh_interval,
            ),
            ("auth.pending_ttl", s.auth.pending_ttl),
            ("auth.principal_cache_ttl", s.auth.principal_cache_ttl),
            (
                "auth.approval_email_interval",
                s.auth.approval_email_interval,
            ),
            ("audit.retention", s.audit.retention),
            ("audit.ip_retention", s.audit.ip_retention),
            (
                "retention.notifications_read_after",
                s.retention.notifications_read_after,
            ),
            (
                "retention.delivery_attempts_after",
                s.retention.delivery_attempts_after,
            ),
            (
                "retention.ai_generation_prompt_after",
                s.retention.ai_generation_prompt_after,
            ),
            (
                "retention.ai_generation_row_after",
                s.retention.ai_generation_row_after,
            ),
            ("ops.check_interval", s.ops.check_interval),
            (
                "ops.due_delivery_overdue_after",
                s.ops.due_delivery_overdue_after,
            ),
            ("jobs.outbox_poll_interval", s.jobs.outbox_poll_interval),
            (
                "jobs.history_retention.succeeded",
                s.jobs.history_retention.succeeded,
            ),
            (
                "jobs.history_retention.failed",
                s.jobs.history_retention.failed,
            ),
            ("http.connect_timeout", s.http.connect_timeout),
            ("http.request_timeout", s.http.request_timeout),
        ] {
            check_duration(key, value, false)?;
        }
        check_duration("auth.oidc.leeway", s.auth.oidc.leeway, true)?;
        check_bucket("rate_limit.unauthenticated", &s.rate_limit.unauthenticated)?;
        check_bucket("rate_limit.provisioning", &s.rate_limit.provisioning)?;
        check_bucket("rate_limit.authenticated", &s.rate_limit.authenticated)?;
        if s.server.body_limit == 0 {
            return Err(ConfigError::Validation(
                "server.body_limit must be at least 1".into(),
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
