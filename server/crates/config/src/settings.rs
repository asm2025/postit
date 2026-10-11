use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::environment::Environment;
use crate::error::ConfigError;
use crate::secret::RedactedSecret;

/// TLS termination for the API listener.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsSettings {
    /// Whether the API terminates TLS itself.
    pub enabled: bool,
    /// PEM certificate chain; required when `enabled`.
    pub cert_path: Option<PathBuf>,
    /// PEM private key; required when `enabled`.
    pub key_path: Option<PathBuf>,
}

/// The built-in static server for the web client bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebSettings {
    /// Whether the server also serves the web client.
    pub enabled: bool,
    /// Directory holding the built web client; required when `enabled`.
    pub root: Option<PathBuf>,
    /// Socket address of a dedicated web listener (for example `0.0.0.0:44315`); unset serves the
    /// web app on the API listener.
    pub bind: Option<String>,
    /// Base URL the web client calls for the API, when it differs from the page origin.
    pub api_base_url: Option<Url>,
}

/// Listener, proxy, and request-handling settings for the API process.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSettings {
    /// Interface address both listeners bind.
    pub host: String,
    /// Port of the API listener.
    pub api_port: u16,
    /// Port of the worker's health and metrics listener.
    pub worker_port: u16,
    /// TLS settings for the API listener.
    pub tls: TlsSettings,
    /// Reverse-proxy addresses or CIDRs whose forwarded headers are trusted for the client IP.
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
    /// Externally visible base URL of the API.
    pub public_url: Url,
    /// How long shutdown waits for in-flight work before exiting.
    #[serde(with = "crate::duration")]
    pub shutdown_timeout: Duration,
    /// Per-request timeout applied to API routes.
    #[serde(with = "crate::duration", default = "default_request_timeout")]
    pub request_timeout: Duration,
    /// Maximum request body size in bytes.
    #[serde(default = "default_body_limit")]
    pub body_limit: usize,
    /// Static web client serving.
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

/// Postgres connection settings; credentials are kept apart from the URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
    /// Where the database is — `postgres://host:port/name`, never credentials, which
    /// are secrets and come from `username` / `password`.
    pub url: Url,
    /// Database user, a secret supplied by the vault.
    pub username: String,
    /// Database password, a secret supplied by the vault.
    pub password: RedactedSecret,
    /// Upper bound on pooled connections.
    pub max_connections: u32,
}

/// When the OIDC `userinfo` endpoint is consulted for profile claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserinfoMode {
    /// Only when the token lacks the claims postit needs.
    Fallback,
    /// On every sign-in.
    Always,
    /// Never; token claims alone are used.
    Never,
}

/// Names of the ID-token claims postit reads, for providers that rename them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcClaimNames {
    /// Claim carrying the email address.
    pub email: String,
    /// Claim carrying whether the email address is verified.
    pub email_verified: String,
    /// Claim carrying the display name.
    pub name: String,
    /// Claim carrying the preferred username.
    pub preferred_username: String,
}

/// OIDC provider and token-verification settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OidcSettings {
    /// Issuer URL; the `iss` claim must match it and discovery starts here.
    pub issuer: Url,
    /// Accepted `aud` values; a token must carry at least one.
    pub audiences: Vec<String>,
    /// OAuth client ID of the web and mobile clients.
    pub client_id: String,
    /// Scopes the clients request at sign-in.
    pub scopes: Vec<String>,
    /// JWT signature algorithms the verifier accepts.
    pub accepted_algorithms: Vec<String>,
    /// Clock-skew tolerance when checking `exp` and `nbf`.
    #[serde(with = "crate::duration")]
    pub leeway: Duration,
    /// How often the signing keys are refreshed.
    #[serde(with = "crate::duration")]
    pub jwks_refresh_interval: Duration,
    /// Claim names to read from tokens.
    pub claim_names: OidcClaimNames,
    /// Whether and when to call the `userinfo` endpoint.
    pub userinfo: UserinfoMode,
    /// Link to the provider's account-management page, shown to users when set.
    pub account_url: Option<Url>,
}

/// First-admin bootstrap; at most one of the two fields may be set. A blank value counts
/// as unset, so a deployment can pass an empty variable (compose `${VAR:-}`) once its
/// first admin exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSettings {
    /// Email address whose first sign-in becomes the initial admin.
    #[serde(default, deserialize_with = "blank_as_none")]
    pub admin_email: Option<String>,
    /// Subject (`sub`) whose first sign-in becomes the initial admin.
    #[serde(default, deserialize_with = "blank_as_none")]
    pub admin_subject: Option<String>,
}

/// Deserializes an optional string, mapping a blank value to `None`.
fn blank_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|s| !s.trim().is_empty()))
}

/// Authentication and account-lifecycle settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSettings {
    /// OIDC provider settings.
    pub oidc: OidcSettings,
    /// First-admin bootstrap.
    pub bootstrap: BootstrapSettings,
    /// How long a user may stay `pending` before being purged.
    #[serde(with = "crate::duration")]
    pub pending_ttl: Duration,
    /// How long a resolved principal is cached.
    #[serde(with = "crate::duration")]
    pub principal_cache_ttl: Duration,
    /// Minimum interval between approval-request emails to admins.
    #[serde(with = "crate::duration")]
    pub approval_email_interval: Duration,
}

/// Audit-log retention and pseudonymisation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditSettings {
    /// How long audit events are kept.
    #[serde(with = "crate::duration")]
    pub retention: Duration,
    /// How long the IP address on an audit event is kept before it is cleared.
    #[serde(with = "crate::duration")]
    pub ip_retention: Duration,
    /// HMAC key used to pseudonymise audit subjects when a user is deleted.
    pub pseudonym_key: RedactedSecret,
}

/// Retention windows for operational data other than the audit log. Reserved:
/// validated at load, not yet read by any job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionSettings {
    /// Reserved: how long after being read a notification is kept.
    #[serde(with = "crate::duration")]
    pub notifications_read_after: Duration,
    /// Reserved: how long delivery attempt rows are kept.
    #[serde(with = "crate::duration")]
    pub delivery_attempts_after: Duration,
    /// Reserved: how long an AI generation's prompt text is kept.
    #[serde(with = "crate::duration")]
    pub ai_generation_prompt_after: Duration,
    /// Reserved: how long an AI generation row is kept.
    #[serde(with = "crate::duration")]
    pub ai_generation_row_after: Duration,
}

/// Settings for the periodic operational health check. Reserved: validated at
/// load, not yet read by any job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpsSettings {
    /// Reserved: how often the check runs.
    #[serde(with = "crate::duration")]
    pub check_interval: Duration,
    /// Reserved: how late a due delivery may be before it is flagged as overdue.
    #[serde(with = "crate::duration")]
    pub due_delivery_overdue_after: Duration,
}

/// A token-bucket rate limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateBucket {
    /// Sustained refill rate, requests per minute.
    pub rate_per_minute: u32,
    /// Bucket capacity, the largest burst allowed.
    pub burst: u32,
}

/// Per-class rate limits applied by the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitSettings {
    /// Limit applied per client IP.
    pub unauthenticated: RateBucket,
    /// Limit for first-sign-in provisioning requests, keyed by client IP.
    pub provisioning: RateBucket,
    /// Limit applied per authenticated user.
    pub authenticated: RateBucket,
}

/// How outbound email is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailTransport {
    /// Deliver through an SMTP server.
    Smtp,
}

/// TLS mode of the SMTP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpTls {
    /// Plain SMTP, no TLS. Development only (a local SMTP tool such as Papercut).
    None,
    /// Upgrade a plain connection with `STARTTLS`.
    Starttls,
    /// Implicit TLS (usually port 465).
    Tls,
}

/// SMTP server connection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmtpSettings {
    /// SMTP server host.
    pub host: String,
    /// SMTP server port.
    pub port: u16,
    /// SMTP username, when the server requires authentication.
    pub username: Option<String>,
    /// SMTP password, when the server requires authentication.
    pub password: Option<RedactedSecret>,
    /// TLS mode of the connection.
    pub tls: SmtpTls,
}

/// Outbound email settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailSettings {
    /// Delivery transport.
    pub transport: MailTransport,
    /// Address (or `Name <address>`) mails are sent from.
    pub from_address: String,
    /// Attempts made to send one email before the job fails for good.
    pub send_email_max_attempts: u32,
    /// SMTP connection settings.
    pub smtp: SmtpSettings,
}

/// How long finished job rows are kept, by outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobHistoryRetention {
    /// Retention for succeeded jobs.
    #[serde(with = "crate::duration")]
    pub succeeded: Duration,
    /// Retention for failed jobs.
    #[serde(with = "crate::duration")]
    pub failed: Duration,
}

/// Cron expressions of the recurring maintenance jobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobSchedules {
    /// Schedule of the job-history purge.
    pub job_history_purge: String,
    /// Schedule of the pending-user purge.
    pub purge_pending_users: String,
    /// Schedule of the audit-log retention sweep.
    pub audit_retention: String,
    /// Schedule of the operational data retention sweep.
    pub data_retention: String,
}

/// Background-job runtime settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobsSettings {
    /// Worker concurrency per queue, keyed by queue name.
    #[serde(default)]
    pub concurrency: HashMap<String, u32>,
    /// How often the worker polls the job outbox.
    #[serde(with = "crate::duration")]
    pub outbox_poll_interval: Duration,
    /// How long finished jobs are kept.
    pub history_retention: JobHistoryRetention,
    /// Recurring job schedules.
    pub schedules: JobSchedules,
}

/// Settings for the shared outbound HTTP client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpSettings {
    /// TCP connect timeout.
    #[serde(with = "crate::duration")]
    pub connect_timeout: Duration,
    /// Whole-request timeout.
    #[serde(with = "crate::duration")]
    pub request_timeout: Duration,
    /// `User-Agent` header sent on outbound requests.
    pub user_agent: String,
    /// Extra PEM CA bundles trusted in addition to the system roots.
    #[serde(default)]
    pub extra_ca_files: Vec<PathBuf>,
}

/// Settings describing the user-facing web and mobile app.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    /// Externally visible base URL of the app, used in emails and redirects.
    pub public_url: Url,
}

/// Cross-origin settings for the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorsSettings {
    /// Origins allowed to call the API from a browser.
    pub allowed_origins: Vec<String>,
}

/// The complete, validated configuration tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Listener and request handling.
    pub server: ServerSettings,
    /// Postgres connection.
    pub database: DatabaseSettings,
    /// Authentication and account lifecycle.
    pub auth: AuthSettings,
    /// Audit log.
    pub audit: AuditSettings,
    /// Data retention windows.
    pub retention: RetentionSettings,
    /// Operational health checks.
    pub ops: OpsSettings,
    /// Rate limits.
    pub rate_limit: RateLimitSettings,
    /// Outbound email.
    pub mail: MailSettings,
    /// Background jobs.
    pub jobs: JobsSettings,
    /// Outbound HTTP client.
    pub http: HttpSettings,
    /// User-facing app.
    pub app: AppSettings,
    /// Cross-origin policy.
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

        if self.audit.pseudonym_key.expose().trim().is_empty() {
            return Err(ConfigError::Validation(
                "audit.pseudonym_key must not be empty".into(),
            ));
        }

        if env != Environment::Development {
            for (key, value) in [
                ("database.password", self.database.password.expose()),
                ("mail.smtp.host", self.mail.smtp.host.as_str()),
                ("mail.from_address", self.mail.from_address.as_str()),
            ] {
                if value.trim().is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "{key} must not be empty outside development"
                    )));
                }
            }
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
