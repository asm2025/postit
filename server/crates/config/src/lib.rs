//! `postit-config`: layered settings (TOML, environment, secrets file) and their validation.
#![warn(missing_docs)]

mod duration;
mod environment;
mod error;
mod loader;
mod secret;
mod settings;

pub use environment::Environment;
pub use error::ConfigError;
pub use loader::{load, load_with};
pub use secret::RedactedSecret;
pub use settings::{
    AppSettings, AuditSettings, AuthSettings, BootstrapSettings, CorsSettings, DEVELOPMENT_KEYS,
    DatabaseSettings, EmailSettings, HttpSettings, JobHistoryRetention, JobSchedules, JobsSettings,
    MailSettings, MailTransport, MediaSettings, OidcClaimNames, OidcSettings, OpsSettings,
    PushSettings, RateBucket, RateLimitSettings, RetentionSettings, ServerSettings, Settings,
    SmtpSettings, SmtpTls, TlsSettings, UserinfoMode, VaultSettings, WebSettings,
};
